# rustc Binary Operator Typing and Lowering

This chapter records how rustc type-checks binary operators and decides
whether they become primitive MIR operations or ordinary trait calls. It is
descriptive research, not a Sage architecture contract. The corresponding
Sage decisions live in the [Numeric Inference Domains and Binary Operator Typing
RFD][numeric-rfd] and the [Body Checking][body-checking] chapter.

The source snapshot is rust-lang/rust commit
`70222712809cd5cc1718ed8995914a1cbacb6b92` (2026-08-31). Paths and function
names below refer to that revision of the checkout at `~/dev/rust`.

## Executive summary

The useful mental model is **trait first, primitive later**:

1. `&&` and `||` are handled directly as lazy boolean operations.
2. Every other binary operator is initially checked as its language-item
   trait method: roughly `Lhs: Add<Rhs>` plus the method's `Output`.
3. In addition to those ordinary `core` impl candidates, rustc hardcodes scalar
   inference shortcuts. For example, it can constrain a shift's result to the
   left type without selecting the right operand's integer type.
4. Numeric variables fall back only after ordinary body constraints and an
   obligation-selection pass have had a chance to resolve them.
5. During writeback, a successfully resolved scalar operation has its recorded
   method-call marker removed. THIR then represents it as a non-overloaded
   binary operation; an operation that retains the marker becomes a call.
6. MIR construction lowers the former to `Rvalue::BinaryOp` and the latter
   through normal call lowering.

Primitive integer implementations are real implementations of `Add`, `Shl`,
and the other operator traits in `core`. The trait solver does not return a
special “builtin addition” candidate. Builtin recognition is a later
elaboration decision based on the resolved scalar operand types.

```mermaid
flowchart TD
    Source[Binary source expression] --> Lazy{&& or ||?}
    Lazy -->|yes| Bool[Check both operands as bool]
    Lazy -->|no| Trait[Create operator-trait method lookup]
    Trait --> Goal[Register trait and normalization obligations]
    Goal --> Hints[Apply rustc-only scalar inference shortcuts]
    Hints --> Fallback[Select obligations, then numeric fallback]
    Fallback --> Writeback{Resolved operands are scalar?}
    Writeback -->|yes| Clear[Clear method-call marker]
    Writeback -->|no| Keep[Keep resolved method call]
    Clear --> Binary[THIR Binary -> MIR BinaryOp]
    Keep --> Call[THIR Call -> MIR call]
```

## Type checking starts from the operator trait

`FnCtxt::check_expr_binop` in
`compiler/rustc_hir_typeck/src/op.rs` separates only short-circuit operators.
It coerces both operands of `&&` and `||` to `bool` and returns `bool`. For all
other binary operators it calls `check_overloaded_binop`, deliberately treating
the expression as potentially overloaded while types are still being inferred.

`check_overloaded_binop` checks the left expression, creates a fresh type for
the right expression, and passes those types to `lookup_op_method`. The fresh
right type also becomes the expected type while checking the right expression,
which permits the usual coercion behavior for cases such as `String + &String`.

`lookup_op_method` maps the syntax to the relevant language item and delegates
to `FnCtxt::lookup_method_for_operator` in
`compiler/rustc_hir_typeck/src/method/mod.rs`. That routine constructs a
trait-reference obligation of the form:

```text
Lhs: OperatorTrait<Rhs>
```

It then instantiates and normalizes the trait method signature, registers the
method's predicates and well-formedness obligations, and returns a
`MethodCallee`. `overloaded_binop_ret_ty` records that method call and uses the
normalized method output as the expression type. Thus `Output` comes from the
same trait method whether the eventual operation is primitive or user-defined.

## rustc adds hardcoded inference shortcuts to ordinary impl proof

After creating the trait operation, rustc additionally calls
`is_builtin_binop` and `enforce_builtin_binop_types` when enough operand shape
is known. These functions hardcode relationships which summarize how `core`'s
scalar impls are arranged:

| family | inference relationship | result |
|---|---|---|
| arithmetic | matching integer types or matching float types | left type |
| bitwise | matching integer types or matching `bool` | left type |
| shifts | both operands are integers; their types may differ | left type |
| comparisons | matching scalar types | `bool` |
| lazy boolean | both operands coerce to `bool` | `bool` |

These shortcuts neither create an impl candidate nor prove the operation. The
operator-trait obligation still uses the ordinary `core` impls and rejects a
scalar pair for which no applicable impl exists. The implementation's
`is_builtin_binop` predicate is intentionally coarse. The actual domain of a successful MIR operation is
documented on `rustc_middle::mir::Rvalue::BinaryOp`: comparisons include
matching booleans, characters, numeric types, raw pointers, and function
pointers; shifts accept independently typed integers; bitwise operations
accept matching integers or booleans; and the remaining arithmetic operations
accept matching integers or floats.

The distinction matters for inference. `1_u32 << 2` can immediately constrain
the result to `u32` even though the right literal has not yet selected one of
the many integer `Shl<Rhs>` implementations. Conversely, applying these
relationships does not replace proving the operator trait.

## Primitive operators use ordinary `core` trait impls

`library/core/src/ops/arith.rs` defines `Add` implementations for each integer
and floating-point primitive. Each same-typed implementation has
`Output = Self`. `library/core/src/ops/bit.rs` defines the shift matrix: every
integer left-hand type implements `Shl<Rhs>` for every integer right-hand
type, again with the left type as `Output`.

Consequently, the trait solver sees ordinary impl candidates for both
primitive and user-defined operations. Nothing in operator method lookup asks
the solver to choose a special primitive candidate. The later type-checking
and lowering stages are responsible for deciding which representation to
emit.

## Numeric variables and fallback

Literal checking in `FnCtxt::check_lit` uses concrete types for suffixed
literals. An unsuffixed integer or float uses a compatible expected numeric
type when one is available; otherwise rustc creates a distinct integer or
floating-point inference variable.

Those distinctions survive canonicalization as `CanonicalVarKind::Int` and
`CanonicalVarKind::Float`. They are not ordinary unrestricted type variables.
Rustc also maintains separate unification tables for the two numeric kinds.

At the end of body checking, `FnCtxt::type_inference_fallback` first selects
obligations that can already make progress, defaults unresolved integer
variables to `i32` and normally defaults unresolved float variables to `f64`,
then selects obligations again if fallback changed anything. The inspected
revision has a compatibility exception that may choose `f32` for a pending
`f32: From<{float}>` obligation and emits a future-compatibility warning. This
exception is compatibility policy, not the basic numeric-inference model.

Fallback runs after the body has contributed its ordinary constraints and
repeat/opaque-type processing, but before cast validation and the final
obligation-selection passes.

## Writeback makes the builtin/call decision

`WritebackCx::fix_scalar_builtin_expr` in
`compiler/rustc_hir_typeck/src/writeback.rs` is the key transition. Type
checking has recorded every non-short-circuit binary operator as a method call.
Once writeback sees scalar operand types, it removes the expression's
type-dependent method definition and generic arguments. For by-reference
comparison operators it also removes the temporary autoref adjustments that
were needed for trait-method checking.

THIR construction in `compiler/rustc_mir_build/src/thir/cx/expr.rs` tests that
marker:

- a remaining method call becomes THIR `ExprKind::Call` through
  `overloaded_operator`;
- `&&` or `||` becomes `ExprKind::LogicalOp`; and
- any other unmarked operator becomes non-overloaded `ExprKind::Binary`.

`compiler/rustc_mir_build/src/builder/expr/as_rvalue.rs` then lowers THIR
`Binary` to MIR `Rvalue::BinaryOp`, adding the appropriate overflow, shift,
division, and remainder checks. The THIR call follows ordinary call lowering.

The resulting examples are:

| source shape | trait checking | completed THIR/MIR path |
|---|---|---|
| `u128 + u128` | resolves `u128: Add<u128, Output = u128>` | `Binary` / `BinaryOp(Add, ...)` |
| `u128 << u8` | resolves `u128: Shl<u8, Output = u128>` | `Binary` / `BinaryOp(Shl, ...)` |
| user type with `impl Add` | resolves that user impl and its `Output` | `Call` to `Add::add` |
| `bool && bool` | no operator-trait lookup | `LogicalOp` with lazy RHS |

These shapes were also corroborated with the nearby installed nightly using
`-Zunpretty=thir-tree`: primitive addition and a mixed-integer shift appeared
as `Binary`, while a user-defined `Add` appeared as `Call`.

## Implications for Sage

The rustc structure suggests the following design constraints; the normative
versions are recorded in the linked Sage RFD:

- Retain an operator-trait obligation and normalized output for every
  non-short-circuit operator. Primitive implementations are ordinary `core`
  impls obtained through Sage's external metadata and normal candidate
  discovery; Sage does not synthesize a builtin operator candidate.
- Sage's first slice does not copy rustc's hardcoded scalar inference
  shortcuts. Any future extraction of facts shared by ambiguous candidates
  must derive them from the actual complete candidate answers through a
  general inference-level design and cannot stand in for proof.
- Sage deliberately does not copy rustc's THIR-time primitive/call split.
  Completed Typed IR keeps every proven non-short-circuit operator as a
  resolved trait-method call, which also represents generic bodies uniformly.
  Future codegen may recognize and specialize primitive calls after generic
  substitutions are concrete.
- Model shift operands independently. Requiring them to be equal would reject
  real primitive impls.
- Preserve integer and float restrictions through canonicalization. Sage may
  store these as transactional equality-class metadata rather than copying
  rustc's separate type variants and unification tables.
- Do not treat rustc's numeric fallback ordering as part of domain narrowing.
  Sage's initial RFD deliberately leaves unresolved numeric classes unresolved;
  a later holistic commitment policy must account for proof-derived trait and
  where-clause constraints before considering any default, including rustc's
  special `f32: From<{float}>` compatibility behavior.

## Source map

| concern | rustc source at the inspected revision |
|---|---|
| binary checking and builtin hints | `compiler/rustc_hir_typeck/src/op.rs`: `check_expr_binop`, `check_overloaded_binop`, `enforce_builtin_binop_types`, `lookup_op_method`, `is_builtin_binop` |
| operator obligation and method output | `compiler/rustc_hir_typeck/src/method/mod.rs`: `lookup_method_for_operator` |
| literal inference variables | `compiler/rustc_hir_typeck/src/fn_ctxt/checks.rs`: `check_lit` |
| numeric canonical kinds | `compiler/rustc_infer/src/infer/canonical/canonicalizer.rs`: `TypeFoldable::try_fold_with` handling for `IntVar` and `FloatVar` |
| numeric fallback | `compiler/rustc_hir_typeck/src/fallback.rs`: `type_inference_fallback`, `fallback_types` |
| fallback placement | `compiler/rustc_hir_typeck/src/lib.rs`: body type-checking driver around `type_inference_fallback` |
| scalar reclassification | `compiler/rustc_hir_typeck/src/writeback.rs`: `fix_scalar_builtin_expr` |
| THIR split | `compiler/rustc_mir_build/src/thir/cx/expr.rs`: binary-expression mirroring |
| THIR representation | `compiler/rustc_middle/src/thir.rs`: `ExprKind::{Call, Binary, LogicalOp}` |
| MIR domain and representation | `compiler/rustc_middle/src/mir/syntax.rs`: `Rvalue::BinaryOp` |
| MIR construction | `compiler/rustc_mir_build/src/builder/expr/as_rvalue.rs`: `build_binary_op` |
| primitive arithmetic impls | `library/core/src/ops/arith.rs`: `add_impl` and sibling macros |
| primitive shift impl matrix | `library/core/src/ops/bit.rs`: `shl_impl`, `shl_impl_all`, and `shr` counterparts |

[body-checking]: ../pipeline/body-checking.md
[numeric-rfd]: ../../rfds/numeric-inference-variables/README.md
