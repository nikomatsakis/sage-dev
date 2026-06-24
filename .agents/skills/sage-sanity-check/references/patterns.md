# Sage Anti-Pattern Check

You are reviewing code that was just generated or modified. Your job is to catch SAGE-specific anti-patterns that the generation pass commonly introduces. Use your tools — read files, grep the codebase. Do not guess. Verify every finding against actual source.

## Phase 1: Research

Read the modified files. Understand what they do and how they fit into the type-checking and IR layers.

## Phase 2: Check each pattern

### Pattern 1: Non-exhaustive matches

Look for wildcard `_` catch-all arms in `match` statements and uses of `if let` on enums defined in this workspace.

**Why this matters:** When new variants are added to an enum, the compiler won't flag non-exhaustive matches. We want the compiler to tell us every place that MAY need updating during a refactor.

**Flag:**
- `_ =>` arms in `match` on workspace-defined enums.
- `if let` on workspace-defined enums (this is a non-exhaustive match in disguise).

**Do NOT flag:**
- `_` or `..` *inside* a pattern (e.g., `Foo(..)`, `Foo(_)`) — ignoring fields is fine.
- Matches on external/std types (`Option`, `Result`, primitives).
- Cases where it is extremely unlikely that a new variant would need different handling at this site.

### Pattern 2: Fresh inference variable on type error

Look for calls to `fresh_ty_var()` (or similar inference-variable constructors) in code paths that are handling a type error.

**Why this matters:** Inference variables mean "I don't know the type yet — resolve it later." They do NOT mean "this is invalid." When a type error is detected, returning a fresh inference variable lets it unify with downstream types, potentially producing confusing secondary errors or silently passing incorrect code.

**Flag:**
- Any `fresh_ty_var()` in a code path reachable only when a type error has occurred (e.g., after failing to resolve a name, accessing a field on a non-ADT, wrong argument count).

**Correct pattern:** Report a diagnostic and return `Ty::Error`.

### Pattern 3: Constructing error types without reporting a diagnostic

Look for fresh construction of error-typed values (`Ty::Error`, `Res::Err`, or similar error variants) where no diagnostic has been reported first.

**Why this matters:** Global invariant — every fresh error-type value in the IR witnesses that at least one diagnostic was emitted. If we construct `Ty::Error` without reporting, the user gets a broken type with no explanation of what went wrong.

**Flag:**
- `Ty::Error` (or `alloc_ty(Ty::Error)`, `Res::Err`, etc.) constructed in a code path where no `cx.report(...)`, `cx.error(...)`, or equivalent diagnostic-emitting call precedes it.

**Do NOT flag:**
- **Propagation** of an existing error: matching on `Res::Err` and returning `Ty::Error` is fine — the error was already reported upstream.
- Code that receives an error from a callee and threads it through — the callee reported it.

## Phase 3: Self-evaluation

Before reporting, critically evaluate each finding:
- Did you verify it against the actual source, or are you assuming?
- Is the `_` arm genuinely a catch-all on a workspace enum, or is it matching an external type?
- Is the `fresh_ty_var()` genuinely on an error path, or is it legitimate inference?
- Is the `Ty::Error` genuinely fresh, or is it propagating an existing error?

Drop findings you can't substantiate.

## Phase 4: Final report

```
## 1. Non-exhaustive matches
[findings or "No issues."]

## 2. Inference variable on error path
[findings or "No issues."]

## 3. Error without diagnostic
[findings or "No issues."]
```

For each finding: file path with line number, the offending code, and a concrete suggestion for what to do instead.
