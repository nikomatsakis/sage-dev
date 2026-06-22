# Rustc Macro Expansion Ordering: Research Report

This report documents how rustc handles attribute macro and derive macro expansion
ordering — specifically, the processing order, what input each macro receives, and
how the item is transformed at each step.

## 1. Processing Order: Outside-In, One Active Attr at a Time

Expansion is iterative. The core loop in `rustc_expand/src/expand.rs`
(`fully_expand_fragment`) repeatedly scans attributes on an item and processes the
first *active* one it finds.

The function `take_first_attr` determines which attribute to process:

1. **`#[cfg]` / `#[cfg_attr]`** always takes highest priority, regardless of position.
2. Among the remaining attributes, it scans **top-to-bottom** and picks the first
   non-builtin attribute name (i.e., proc-macro attribute).
3. `derive` is a builtin attribute name, so `take_first_attr` skips it. Derives
   are handled through a separate path after all attr macros above them have been
   resolved.

The critical check in `take_first_attr`:
```rust
} else if attr_pos.is_none()
    && !name.is_some_and(rustc_feature::is_builtin_attr_name)
{
    attr_pos = Some(pos); // only picks non-builtin attrs
}
```

## 2. Attribute Macros: Replace the Item

When an attr macro is selected:
- **Its own line is removed** from the item's attribute list (via `attrs.remove(pos)`).
- **All other attributes remain** — inert ones, derives, and even attr macros below it.
- The proc macro receives two `TokenStream` arguments: the attr's own arguments, and
  the item (with remaining attrs).
- The proc macro **replaces** the entire annotated item with its output.
- After replacement, expansion restarts from scratch on the new item.

### Example

```rust
#[attr_macro1]
#[attr_macro2]
#[inert1]
#[derive(Clone)]
#[inert2]
#[attr_macro3]
struct Foo {}
```

**Round 1**: `attr_macro1` is first active attr. It receives:
```rust
#[attr_macro2]
#[inert1]
#[derive(Clone)]
#[inert2]
#[attr_macro3]
struct Foo {}
```

Suppose it passes through unchanged (returns the same tokens).

**Round 2**: `attr_macro2` is first active. It receives:
```rust
#[inert1]
#[derive(Clone)]
#[inert2]
#[attr_macro3]
struct Foo {}
```

**Round 3**: `attr_macro3` is next active (derives are skipped). It receives:
```rust
#[inert1]
#[derive(Clone)]
#[inert2]
struct Foo {}
```

## 3. Derives: Append New Items

After all attr macros have been processed, derives run.

- The `#[derive(...)]` line itself is **stripped** from the item.
- All other attributes (inert ones above and below) are **kept**.
- `#[cfg]`/`#[cfg_attr]` are eagerly evaluated via `cfg_eval` before derives see the
  item (in `rustc_builtin_macros/src/derive.rs`).
- Each derive receives the resulting item as its input.
- Derives **append** new items (typically `impl` blocks) alongside the original.
  They do not replace it.

### Example (continuing from above)

**Round 4**: No more attr macros. `Clone` derive gets:
```rust
#[inert1]
#[inert2]
struct Foo {}
```

## 4. Multiple Derives in One Attribute

For `#[derive(A, B, C)]`:
- All three are expanded **independently**.
- Each receives the **same clone** of the item.
- Output of one derive does NOT affect the input of another.

From `rustc_builtin_macros/src/derive.rs`:
```rust
[first, others @ ..] => {
    first.item = cfg_eval(sess, features, item.clone(), ...);
    for other in others {
        other.item = first.item.clone();
    }
}
```

## 5. Multiple `#[derive(...)]` Attributes

If an item has multiple derive attributes:
```rust
#[derive(Clone)]
#[derive(Debug)]
struct Foo {}
```

They are collected together. The item with all `#[derive(...)]` lines removed is
cloned to each derive. Each still sees all non-derive attributes.

## 6. The Final Item

After all expansion:
- Inert attributes (`#[inline]`, `#[repr(C)]`, `#[allow(...)]`, doc comments) remain
  on the item permanently.
- `#[derive(...)]` lines are gone.
- The expanded `impl` blocks from derives exist as sibling items.

## 7. Implications for Sage

Our `expand_attribute_macros_and_derives` function should:

1. Scan attrs top-to-bottom for the first active attr (non-inert, non-derive).
2. If attr macro found: serialize item with that attr removed, expand, re-parse output,
   recursively process resulting items.
3. If no attr macros remain: collect all derives, serialize item with all derive lines
   removed, expand each derive independently against that text, parse outputs.
4. Push the original item (with derives stripped) + all derive outputs into entries.

The key difference from a naive skip-N model: attr macros consume and replace, while
derives append alongside. And the input to a derive is the item with *all* derive
lines removed, not just the current one.

## Key Source Files

| File | Key Function | Role |
|------|-------------|------|
| `rustc_expand/src/expand.rs` | `take_first_attr` | Selects which attribute to process next |
| `rustc_expand/src/expand.rs` | `fully_expand_fragment` | Main expansion loop |
| `rustc_expand/src/base.rs` | `SyntaxExtensionKind::Attr` | Attr macro signature |
| `rustc_expand/src/base.rs` | `DeriveResolution` | Per-derive state |
| `rustc_builtin_macros/src/derive.rs` | `Expander::expand` | Builtin `#[derive]` handler |
| `rustc_builtin_macros/src/cfg_eval.rs` | `cfg_eval` | Eager cfg evaluation before derives |
| `rustc_expand/src/proc_macro.rs` | `DeriveProcMacro::expand` | Invokes derive client |
