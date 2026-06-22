# tree-sitter-rust Grammar Reference

This document shows how `tree-sitter-rust` (v0.24) parses various Rust grammar
constructs. Each section shows a Rust snippet and the resulting named-node tree.
Leaf nodes show their text in quotes. Field names are shown as `field: node_kind`.

Generated with tree-sitter 0.25 / tree-sitter-rust 0.24.

---

## macro_rules: single arm, empty pattern

```rust
macro_rules! my_mac {
    () => { println!("hello"); };
}
```

```
source_file
  macro_definition
    name: identifier "my_mac"
    macro_rule
      left: token_tree_pattern "()"
      right: token_tree
        identifier "println"
        token_tree
          string_literal
            string_content "hello"
```

**Key observations:**
- `macro_rules!` becomes `macro_definition` (not `macro_invocation`)
- Each arm is a `macro_rule` with `left` (pattern) and `right` (body) fields
- An empty pattern `()` is a leaf `token_tree_pattern`
- The RHS body is an *opaque* `token_tree` — tree-sitter does NOT parse the expansion body as Rust; identifiers and literals are recognized but structure (e.g., `println!(...)` as a macro call) is lost

---

## macro_rules: multiple arms with fragment specifiers

```rust
macro_rules! my_mac {
    ($x:expr) => { $x + 1 };
    ($x:expr, $y:expr) => { $x + $y };
}
```

```
source_file
  macro_definition
    name: identifier "my_mac"
    macro_rule
      left: token_tree_pattern
        token_binding_pattern
          name: metavariable "$x"
          type: fragment_specifier "expr"
      right: token_tree
        metavariable "$x"
        integer_literal "1"
    macro_rule
      left: token_tree_pattern
        token_binding_pattern
          name: metavariable "$x"
          type: fragment_specifier "expr"
        token_binding_pattern
          name: metavariable "$y"
          type: fragment_specifier "expr"
      right: token_tree
        metavariable "$x"
        metavariable "$y"
```

**Key observations:**
- Fragment specifiers (`$x:expr`) become `token_binding_pattern` with `name` (a `metavariable`) and `type` (a `fragment_specifier`)
- In the RHS, `$x` appears as a `metavariable` node
- Operators like `+` are anonymous nodes (not shown in named-only trees)

---

## macro_rules: repetition patterns

```rust
macro_rules! vec_like {
    ( $( $x:expr ),* ) => {
        {
            let mut temp = Vec::new();
            $( temp.push($x); )*
            temp
        }
    };
}
```

```
source_file
  macro_definition
    name: identifier "vec_like"
    macro_rule
      left: token_tree_pattern
        token_repetition_pattern
          token_binding_pattern
            name: metavariable "$x"
            type: fragment_specifier "expr"
      right: token_tree
        token_tree
          mutable_specifier "mut"
          identifier "temp"
          identifier "Vec"
          identifier "new"
          token_tree "()"
          token_repetition
            identifier "temp"
            identifier "push"
            token_tree
              metavariable "$x"
          identifier "temp"
```

**Key observations:**
- `$( ... ),*` in the LHS becomes `token_repetition_pattern`
- `$( ... )*` in the RHS becomes `token_repetition`
- The separator (`,`) and quantifier (`*`) are anonymous children
- Nested braces `{ { ... } }` produce nested `token_tree` nodes

---

## macro_rules: tt muncher

```rust
macro_rules! count {
    () => { 0 };
    ($head:tt $($tail:tt)*) => { 1 + count!($($tail)*) };
}
```

```
source_file
  macro_definition
    name: identifier "count"
    macro_rule
      left: token_tree_pattern "()"
      right: token_tree
        integer_literal "0"
    macro_rule
      left: token_tree_pattern
        token_binding_pattern
          name: metavariable "$head"
          type: fragment_specifier "tt"
        token_repetition_pattern
          token_binding_pattern
            name: metavariable "$tail"
            type: fragment_specifier "tt"
      right: token_tree
        integer_literal "1"
        identifier "count"
        token_tree
          token_repetition
            metavariable "$tail"
```

**Key observations:**
- `$head:tt` uses fragment specifier `tt`
- `$($tail:tt)*` nests `token_binding_pattern` inside `token_repetition_pattern`
- In the RHS, `count!(...)` is NOT parsed as a macro invocation — it's just `identifier` + `token_tree`

---

## macro invocation: paren style in expression position

```rust
fn f() { println!("x = {}", x); }
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      expression_statement
        macro_invocation
          macro: identifier "println"
          token_tree
            string_literal
              string_content "x = {}"
            identifier "x"
```

**Key observations:**
- `println!(...)` is `macro_invocation` with `macro` field = the name
- Arguments are an opaque `token_tree` (string literals and identifiers are recognized)
- The `!` is anonymous

---

## macro invocation: bracket style as expression

```rust
fn f() { let v = vec![1, 2, 3]; }
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: identifier "v"
        value: macro_invocation
          macro: identifier "vec"
          token_tree
            integer_literal "1"
            integer_literal "2"
            integer_literal "3"
```

**Key observations:**
- `vec![...]` uses brackets but is still `macro_invocation`
- The delimiter style (`()`, `[]`, `{}`) is encoded in the anonymous punctuation, not the node kind

---

## macro invocation: brace style as item

```rust
thread_local! { static FOO: u32 = 42; }
```

```
source_file
  macro_invocation
    macro: identifier "thread_local"
    token_tree
      identifier "FOO"
      primitive_type "u32"
      integer_literal "42"
```

**Key observations:**
- Brace-delimited macro invocations at item position are parsed identically
- The `static` keyword inside the token tree is anonymous (not a named node in this context)

---

## match: basic patterns including range and guard

```rust
fn f(x: i32) -> &'static str {
    match x {
        0 => "zero",
        1..=9 => "digit",
        n if n < 0 => "neg",
        _ => "other",
    }
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters
      parameter
        pattern: identifier "x"
        type: primitive_type "i32"
    return_type: reference_type
      lifetime
        identifier "static"
      type: primitive_type "str"
    body: block
      expression_statement
        match_expression
          value: identifier "x"
          body: match_block
            match_arm
              pattern: match_pattern
                integer_literal "0"
              value: string_literal
                string_content "zero"
            match_arm
              pattern: match_pattern
                range_pattern
                  left: integer_literal "1"
                  right: integer_literal "9"
              value: string_literal
                string_content "digit"
            match_arm
              pattern: match_pattern
                identifier "n"
                condition: binary_expression
                  left: identifier "n"
                  right: integer_literal "0"
              value: string_literal
                string_content "neg"
            match_arm
              pattern: match_pattern "_"
              value: string_literal
                string_content "other"
```

**Key observations:**
- `match` produces `match_expression` with `value` and `body: match_block`
- Each arm is `match_arm` with `pattern: match_pattern` and `value`
- Range patterns (`1..=9`) are `range_pattern` with `left`/`right`
- Match guards (`if n < 0`) are a `condition` field on `match_pattern`
- Wildcard `_` makes `match_pattern` a leaf

---

## match: struct/enum destructuring with guard

```rust
fn f(val: Option<(i32, i32)>) {
    match val {
        Some((x, y)) if x > 0 => {}
        Some((0, _)) => {}
        None => {}
    }
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters
      parameter
        pattern: identifier "val"
        type: generic_type
          type: type_identifier "Option"
          type_arguments: type_arguments
            tuple_type
              primitive_type "i32"
              primitive_type "i32"
    body: block
      expression_statement
        match_expression
          value: identifier "val"
          body: match_block
            match_arm
              pattern: match_pattern
                tuple_struct_pattern
                  type: identifier "Some"
                  tuple_pattern
                    identifier "x"
                    identifier "y"
                condition: binary_expression
                  left: identifier "x"
                  right: integer_literal "0"
              value: block "{}"
            match_arm
              pattern: match_pattern
                tuple_struct_pattern
                  type: identifier "Some"
                  tuple_pattern
                    integer_literal "0"
              value: block "{}"
            match_arm
              pattern: match_pattern
                identifier "None"
              value: block "{}"
```

**Key observations:**
- `Some((x, y))` is `tuple_struct_pattern` with `type: identifier "Some"` and a nested `tuple_pattern`
- The guard is a `condition` field on `match_pattern`
- `None` is just an `identifier` — tree-sitter doesn't distinguish constructors from bindings

---

## match: or-patterns

```rust
fn f(x: Result<i32, i32>) {
    match x {
        Ok(0) | Err(0) => {}
        Ok(n) | Err(n) => { let _ = n; }
    }
}
```

```
source_file
  function_item
    ...
    body: block
      expression_statement
        match_expression
          value: identifier "x"
          body: match_block
            match_arm
              pattern: match_pattern
                or_pattern
                  tuple_struct_pattern
                    type: identifier "Ok"
                    integer_literal "0"
                  tuple_struct_pattern
                    type: identifier "Err"
                    integer_literal "0"
              value: block "{}"
            match_arm
              pattern: match_pattern
                or_pattern
                  tuple_struct_pattern
                    type: identifier "Ok"
                    identifier "n"
                  tuple_struct_pattern
                    type: identifier "Err"
                    identifier "n"
              value: block
                let_declaration
                  value: identifier "n"
```

**Key observations:**
- `Ok(0) | Err(0)` is an `or_pattern` containing multiple `tuple_struct_pattern` alternatives
- Each alternative is a direct child of `or_pattern`

---

## block: tail expression (no semicolon) — returns value

```rust
fn f() -> i32 {
    let x = 1;
    x + 1
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    return_type: primitive_type "i32"
    body: block
      let_declaration
        pattern: identifier "x"
        value: integer_literal "1"
      binary_expression
        left: identifier "x"
        right: integer_literal "1"
```

**Key observations:**
- A tail expression (no semicolon) is a **direct child of `block`** — NOT wrapped in `expression_statement`
- This is THE critical distinction: `expression_statement` = discarded, bare expression = tail/return value

---

## block: expression statement (with semicolon) — discarded

```rust
fn f() {
    let x = 1;
    x + 1;
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: identifier "x"
        value: integer_literal "1"
      expression_statement
        binary_expression
          left: identifier "x"
          right: integer_literal "1"
```

**Key observations:**
- With a semicolon, the expression is wrapped in `expression_statement`
- Compare directly with the tail-expression case above

---

## block: if-else as tail expression

```rust
fn f(b: bool) -> i32 {
    if b { 1 } else { 2 }
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters
      parameter
        pattern: identifier "b"
        type: primitive_type "bool"
    return_type: primitive_type "i32"
    body: block
      expression_statement
        if_expression
          condition: identifier "b"
          consequence: block
            integer_literal "1"
          alternative: else_clause
            block
              integer_literal "2"
```

**Key observations:**
- **Surprising:** `if` in tail position is STILL wrapped in `expression_statement`! This is a tree-sitter-rust quirk — `if_expression`, `match_expression`, `loop_expression` etc. are always `expression_statement` children even in tail position
- To detect "tail expression" for these, check: is it the last child of `block` AND does it lack a trailing `;`?
- The inner blocks (`{ 1 }`, `{ 2 }`) have their values as bare children (true tail expressions)

---

## block: if-else as statement (with semicolon)

```rust
fn f(b: bool) {
    if b { 1 } else { 2 };
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters
      parameter
        pattern: identifier "b"
        type: primitive_type "bool"
    body: block
      expression_statement
        if_expression
          condition: identifier "b"
          consequence: block
            integer_literal "1"
          alternative: else_clause
            block
              integer_literal "2"
      empty_statement ";"
```

**Key observations:**
- A trailing `;` after `if/else` produces a separate `empty_statement` sibling — it does NOT move into the `expression_statement`
- This means: `if` with semicolon = `expression_statement` + `empty_statement`; without semicolon = just `expression_statement`
- To distinguish tail-if from discarded-if, check for an `empty_statement` immediately following

---

## enum: unit, tuple, and struct variants

```rust
enum Foo {
    Unit,
    Tuple(i32, String),
    Struct { x: i32, y: String },
}
```

```
source_file
  enum_item
    name: type_identifier "Foo"
    body: enum_variant_list
      enum_variant
        name: identifier "Unit"
      enum_variant
        name: identifier "Tuple"
        body: ordered_field_declaration_list
          type: primitive_type "i32"
          type: type_identifier "String"
      enum_variant
        name: identifier "Struct"
        body: field_declaration_list
          field_declaration
            name: field_identifier "x"
            type: primitive_type "i32"
          field_declaration
            name: field_identifier "y"
            type: type_identifier "String"
```

**Key observations:**
- All three variant kinds share the `enum_variant` node; distinguished by presence/kind of `body`
  - Unit: no `body` field
  - Tuple: `body: ordered_field_declaration_list` (types only, no names)
  - Struct: `body: field_declaration_list` with `field_declaration` children

---

## enum: with explicit discriminants

```rust
enum Color {
    Red = 1,
    Green = 2,
    Blue = 3,
}
```

```
source_file
  enum_item
    name: type_identifier "Color"
    body: enum_variant_list
      enum_variant
        name: identifier "Red"
        value: integer_literal "1"
      enum_variant
        name: identifier "Green"
        value: integer_literal "2"
      enum_variant
        name: identifier "Blue"
        value: integer_literal "3"
```

**Key observations:**
- Discriminant values use the `value` field on `enum_variant`

---

## struct: named fields

```rust
struct Point {
    x: f64,
    y: f64,
}
```

```
source_file
  struct_item
    name: type_identifier "Point"
    body: field_declaration_list
      field_declaration
        name: field_identifier "x"
        type: primitive_type "f64"
      field_declaration
        name: field_identifier "y"
        type: primitive_type "f64"
```

---

## struct: tuple struct

```rust
struct Wrapper(pub i32, String);
```

```
source_file
  struct_item
    name: type_identifier "Wrapper"
    body: ordered_field_declaration_list
      visibility_modifier "pub"
      type: primitive_type "i32"
      type: type_identifier "String"
```

**Key observations:**
- Tuple structs use `ordered_field_declaration_list` (same as tuple enum variants)
- Visibility modifiers appear as children but are NOT per-field — they precede the `type` they modify

---

## struct: unit struct

```rust
struct Marker;
```

```
source_file
  struct_item
    name: type_identifier "Marker"
```

**Key observations:**
- Unit structs have no `body` field at all

---

## trait: associated type + default method

```rust
trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
    fn count(self) -> usize { 0 }
}
```

```
source_file
  trait_item
    name: type_identifier "Iterator"
    body: declaration_list
      associated_type
        name: type_identifier "Item"
      function_signature_item
        name: identifier "next"
        parameters: parameters
          self_parameter
            mutable_specifier "mut"
            self "self"
        return_type: generic_type
          type: type_identifier "Option"
          type_arguments: type_arguments
            scoped_type_identifier
              path: identifier "Self"
              name: type_identifier "Item"
      function_item
        name: identifier "count"
        parameters: parameters
          self_parameter
            self "self"
        return_type: primitive_type "usize"
        body: block
          integer_literal "0"
```

**Key observations:**
- Trait body is `declaration_list`
- Methods without body → `function_signature_item`; with body → `function_item`
- `&mut self` is `self_parameter` with `mutable_specifier`
- `Self::Item` is `scoped_type_identifier` with `path: identifier "Self"`

---

## impl: inherent with lifetime

```rust
impl<'a> Foo<'a> {
    fn new(x: &'a str) -> Self { Foo { x } }
}
```

```
source_file
  impl_item
    type_parameters: type_parameters
      lifetime_parameter
        name: lifetime
          identifier "a"
    type: generic_type
      type: type_identifier "Foo"
      type_arguments: type_arguments
        lifetime
          identifier "a"
    body: declaration_list
      function_item
        name: identifier "new"
        parameters: parameters
          parameter
            pattern: identifier "x"
            type: reference_type
              lifetime
                identifier "a"
              type: primitive_type "str"
        return_type: type_identifier "Self"
        body: block
          struct_expression
            name: type_identifier "Foo"
            body: field_initializer_list
              shorthand_field_initializer
                identifier "x"
```

**Key observations:**
- `impl<'a>` → `type_parameters` containing `lifetime_parameter`
- Lifetimes are `lifetime` > `identifier` (the `'` is anonymous)
- Struct literal `Foo { x }` is `struct_expression` with `shorthand_field_initializer`

---

## impl: trait for type

```rust
impl Display for Point {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}
```

```
source_file
  impl_item
    trait: type_identifier "Display"
    type: type_identifier "Point"
    body: declaration_list
      function_item
        name: identifier "fmt"
        parameters: parameters
          self_parameter
            self "self"
          parameter
            pattern: identifier "f"
            type: reference_type
              mutable_specifier "mut"
              type: generic_type
                type: type_identifier "Formatter"
                type_arguments: type_arguments
                  lifetime
                    identifier "_"
        return_type: type_identifier "Result"
        body: block
          macro_invocation
            macro: identifier "write"
            token_tree
              identifier "f"
              string_literal
                string_content "({}, {})"
              self "self"
              identifier "x"
              self "self"
              identifier "y"
```

**Key observations:**
- Trait impl has both `trait` and `type` fields (inherent impl only has `type`)
- `&self` is `self_parameter` (no `mutable_specifier`)
- `self.x` inside a `token_tree` is decomposed into `self` + `identifier "x"` (no `field_expression`)

---

## use: nested groups

```rust
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
};
```

```
source_file
  use_declaration
    argument: scoped_use_list
      path: identifier "std"
      list: use_list
        scoped_use_list
          path: identifier "collections"
          list: use_list
            identifier "HashMap"
            identifier "HashSet"
        scoped_use_list
          path: identifier "io"
          list: use_list
            self "self"
            identifier "Read"
            identifier "Write"
```

**Key observations:**
- `use std::{...}` → `scoped_use_list` with `path` and `list: use_list`
- Nesting produces recursive `scoped_use_list`
- `self` in `use std::io::{self, ...}` is a named `self` node

---

## use: glob and rename

```rust
use std::collections::*;
use std::io::Result as IoResult;
```

```
source_file
  use_declaration
    argument: use_wildcard
      scoped_identifier
        path: identifier "std"
        name: identifier "collections"
  use_declaration
    argument: use_as_clause
      path: scoped_identifier
        path: scoped_identifier
          path: identifier "std"
          name: identifier "io"
        name: identifier "Result"
      alias: identifier "IoResult"
```

**Key observations:**
- Glob import → `use_wildcard` (the `*` is anonymous)
- Rename → `use_as_clause` with `path` and `alias`
- Multi-segment paths are nested `scoped_identifier`

---

## closure: typed, move, and inferred

```rust
fn f() {
    let a = |x| x + 1;
    let b = |x: i32| -> i32 { x + 1 };
    let c = move || println!("hi");
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: identifier "a"
        value: closure_expression
          parameters: closure_parameters
            identifier "x"
          body: binary_expression
            left: identifier "x"
            right: integer_literal "1"
      let_declaration
        pattern: identifier "b"
        value: closure_expression
          parameters: closure_parameters
            parameter
              pattern: identifier "x"
              type: primitive_type "i32"
          return_type: primitive_type "i32"
          body: block
            binary_expression
              left: identifier "x"
              right: integer_literal "1"
      let_declaration
        pattern: identifier "c"
        value: closure_expression
          parameters: closure_parameters "||"
          body: macro_invocation
            macro: identifier "println"
            token_tree
              string_literal
                string_content "hi"
```

**Key observations:**
- All closures are `closure_expression`
- Inferred params: bare `identifier` inside `closure_parameters`
- Typed params: `parameter` with `pattern` + `type` (same as fn params)
- `move` is anonymous; `||` with no params makes `closure_parameters` a leaf
- Body without braces is a bare expression; with braces is a `block`

---

## async fn and .await

```rust
async fn fetch(url: &str) -> String {
    let resp = client.get(url).await;
    resp.text().await
}
```

```
source_file
  function_item
    function_modifiers "async"
    name: identifier "fetch"
    parameters: parameters
      parameter
        pattern: identifier "url"
        type: reference_type
          type: primitive_type "str"
    return_type: type_identifier "String"
    body: block
      let_declaration
        pattern: identifier "resp"
        value: await_expression
          call_expression
            function: field_expression
              value: identifier "client"
              field: field_identifier "get"
            arguments: arguments
              identifier "url"
      await_expression
        call_expression
          function: field_expression
            value: identifier "resp"
            field: field_identifier "text"
          arguments: arguments "()"
```

**Key observations:**
- `async` is `function_modifiers` (like `unsafe`, `const`)
- `.await` wraps the expression in `await_expression` (child is the awaited expr)
- `.await` in tail position is a bare `await_expression`; with `;` it would be in `expression_statement`

---

## where clause: multiple bounds

```rust
fn process<T, U>(t: T) -> U
where
    T: Iterator<Item = U> + Send + 'static,
    U: Display + Clone,
{
    todo!()
}
```

```
source_file
  function_item
    name: identifier "process"
    type_parameters: type_parameters
      type_parameter
        name: type_identifier "T"
      type_parameter
        name: type_identifier "U"
    parameters: parameters
      parameter
        pattern: identifier "t"
        type: type_identifier "T"
    return_type: type_identifier "U"
    where_clause
      where_predicate
        left: type_identifier "T"
        bounds: trait_bounds
          generic_type
            type: type_identifier "Iterator"
            type_arguments: type_arguments
              type_binding
                name: type_identifier "Item"
                type: type_identifier "U"
          type_identifier "Send"
          lifetime
            identifier "static"
      where_predicate
        left: type_identifier "U"
        bounds: trait_bounds
          type_identifier "Display"
          type_identifier "Clone"
    body: block
      macro_invocation
        macro: identifier "todo"
        token_tree "()"
```

**Key observations:**
- `where_clause` contains `where_predicate` entries with `left` (type) and `bounds: trait_bounds`
- `trait_bounds` children are the individual bounds (traits, lifetimes)
- Associated type constraints (`Item = U`) are `type_binding` inside `type_arguments`
- Lifetime bounds (`'static`) are `lifetime` nodes inside `trait_bounds`

---

## const and static items

```rust
const MAX: usize = 100;
static COUNTER: AtomicUsize = AtomicUsize::new(0);
static mut BUFFER: [u8; 1024] = [0; 1024];
```

```
source_file
  const_item
    name: identifier "MAX"
    type: primitive_type "usize"
    value: integer_literal "100"
  static_item
    name: identifier "COUNTER"
    type: type_identifier "AtomicUsize"
    value: call_expression
      function: scoped_identifier
        path: identifier "AtomicUsize"
        name: identifier "new"
      arguments: arguments
        integer_literal "0"
  static_item
    mutable_specifier "mut"
    name: identifier "BUFFER"
    type: array_type
      element: primitive_type "u8"
      length: integer_literal "1024"
    value: array_expression
      integer_literal "0"
      length: integer_literal "1024"
```

**Key observations:**
- `const` → `const_item`; `static` → `static_item`
- `static mut` has a `mutable_specifier` child
- Array types `[u8; 1024]` are `array_type` with `element` and `length`
- Array repeat expressions `[0; 1024]` are `array_expression` with `length`

---

## type alias

```rust
type Pair<T> = (T, T);
type Result<T> = std::result::Result<T, MyError>;
```

```
source_file
  type_item
    name: type_identifier "Pair"
    type_parameters: type_parameters
      type_parameter
        name: type_identifier "T"
    type: tuple_type
      type_identifier "T"
      type_identifier "T"
  type_item
    name: type_identifier "Result"
    type_parameters: type_parameters
      type_parameter
        name: type_identifier "T"
    type: generic_type
      type: scoped_type_identifier
        path: scoped_identifier
          path: identifier "std"
          name: identifier "result"
        name: type_identifier "Result"
      type_arguments: type_arguments
        type_identifier "T"
        type_identifier "MyError"
```

**Key observations:**
- Type aliases are `type_item` with `name`, optional `type_parameters`, and `type`
- Qualified types use `scoped_type_identifier` (type position) vs `scoped_identifier` (value position)

---

## attributes: inner and outer

```rust
#![allow(dead_code)]

#[derive(Debug, Clone)]
#[repr(C)]
struct Foo {
    #[cfg(target_os = "linux")]
    x: i32,
}
```

```
source_file
  inner_attribute_item
    attribute
      identifier "allow"
      arguments: token_tree
        identifier "dead_code"
  attribute_item
    attribute
      identifier "derive"
      arguments: token_tree
        identifier "Debug"
        identifier "Clone"
  attribute_item
    attribute
      identifier "repr"
      arguments: token_tree
        identifier "C"
  struct_item
    name: type_identifier "Foo"
    body: field_declaration_list
      attribute_item
        attribute
          identifier "cfg"
          arguments: token_tree
            identifier "target_os"
            string_literal
              string_content "linux"
      field_declaration
        name: field_identifier "x"
        type: primitive_type "i32"
```

**Key observations:**
- `#![...]` → `inner_attribute_item`; `#[...]` → `attribute_item`
- Attribute arguments are opaque `token_tree`
- Outer attributes on items appear as **preceding siblings** (not children of the item)
- Field-level attributes appear as children of `field_declaration_list` before their field
- Multiple `#[...]` on one item produce multiple sibling `attribute_item` nodes

---

## patterns: destructuring in let bindings

```rust
fn f() {
    let (a, b) = (1, 2);
    let Point { x, y } = point;
    let [first, rest @ ..] = slice;
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: tuple_pattern
          identifier "a"
          identifier "b"
        value: tuple_expression
          integer_literal "1"
          integer_literal "2"
      let_declaration
        pattern: struct_pattern
          type: type_identifier "Point"
          field_pattern
            name: shorthand_field_identifier "x"
          field_pattern
            name: shorthand_field_identifier "y"
        value: identifier "point"
      let_declaration
        pattern: slice_pattern
          identifier "first"
          captured_pattern
            identifier "rest"
            remaining_field_pattern ".."
        value: identifier "slice"
```

**Key observations:**
- Pattern kinds: `tuple_pattern`, `struct_pattern`, `slice_pattern`
- Struct shorthand `{ x, y }` uses `field_pattern` > `shorthand_field_identifier`
- `rest @ ..` is `captured_pattern` containing the binding and `remaining_field_pattern`

---

## loops: for, while-let, loop-break-value

```rust
fn f() {
    for i in 0..10 { }
    while let Some(x) = iter.next() { }
    let val = loop { break 42; };
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      expression_statement
        for_expression
          pattern: identifier "i"
          value: range_expression
            integer_literal "0"
            integer_literal "10"
          body: block "{ }"
      expression_statement
        while_expression
          condition: let_condition
            pattern: tuple_struct_pattern
              type: identifier "Some"
              identifier "x"
            value: call_expression
              function: field_expression
                value: identifier "iter"
                field: field_identifier "next"
              arguments: arguments "()"
          body: block "{ }"
      let_declaration
        pattern: identifier "val"
        value: loop_expression
          body: block
            expression_statement
              break_expression
                integer_literal "42"
```

**Key observations:**
- `for` → `for_expression` with `pattern`, `value` (the iterator), `body`
- `while let` → `while_expression` with `condition: let_condition`
- `loop` → `loop_expression` with `body`
- `break 42` → `break_expression` with the value as child

---

## turbofish syntax

```rust
fn f() {
    let x = Vec::<i32>::new();
    let y = "42".parse::<i32>().unwrap();
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: identifier "x"
        value: call_expression
          function: scoped_identifier
            path: generic_type
              type: type_identifier "Vec"
              type_arguments: type_arguments
                primitive_type "i32"
            name: identifier "new"
          arguments: arguments "()"
      let_declaration
        pattern: identifier "y"
        value: call_expression
          function: field_expression
            value: call_expression
              function: generic_function
                function: field_expression
                  value: string_literal
                    string_content "42"
                  field: field_identifier "parse"
                type_arguments: type_arguments
                  primitive_type "i32"
              arguments: arguments "()"
            field: field_identifier "unwrap"
          arguments: arguments "()"
```

**Key observations:**
- `Vec::<i32>::new()` — the turbofish on a type produces `generic_type` as the `path` of a `scoped_identifier`
- `"42".parse::<i32>()` — turbofish on a method call produces `generic_function` wrapping the `field_expression`
- `generic_function` has `function` (the base expression) and `type_arguments`

---

## raw identifiers (r#ident)

```rust
fn r#match(r#type: i32) -> i32 { r#type }
```

```
source_file
  function_item
    name: identifier "r#match"
    parameters: parameters
      parameter
        pattern: identifier "r#type"
        type: primitive_type "i32"
    return_type: primitive_type "i32"
    body: block
      identifier "r#type"
```

**Key observations:**
- Raw identifiers are plain `identifier` nodes — the `r#` prefix is part of the text
- No special node kind distinguishes raw identifiers from regular ones

---

## unsafe fn and unsafe block

```rust
unsafe fn danger() -> *const u8 {
    let p: *const u8 = std::ptr::null();
    unsafe { *p }
}
```

```
source_file
  function_item
    function_modifiers "unsafe"
    name: identifier "danger"
    parameters: parameters "()"
    return_type: pointer_type
      type: primitive_type "u8"
    body: block
      let_declaration
        pattern: identifier "p"
        type: pointer_type
          type: primitive_type "u8"
        value: call_expression
          function: scoped_identifier
            path: scoped_identifier
              path: identifier "std"
              name: identifier "ptr"
            name: identifier "null"
          arguments: arguments "()"
      expression_statement
        unsafe_block
          block
            unary_expression
              identifier "p"
```

**Key observations:**
- `unsafe fn` → `function_modifiers "unsafe"` (same slot as `async`, `const`)
- `unsafe { ... }` → `unsafe_block` containing a `block`
- `*const u8` in type position → `pointer_type`; `*p` (deref) → `unary_expression`

---

## visibility modifiers

```rust
pub struct Foo {
    pub x: i32,
    pub(crate) y: i32,
    pub(super) z: i32,
    w: i32,
}
```

```
source_file
  struct_item
    visibility_modifier "pub"
    name: type_identifier "Foo"
    body: field_declaration_list
      field_declaration
        visibility_modifier "pub"
        name: field_identifier "x"
        type: primitive_type "i32"
      field_declaration
        visibility_modifier
          crate "crate"
        name: field_identifier "y"
        type: primitive_type "i32"
      field_declaration
        visibility_modifier
          super "super"
        name: field_identifier "z"
        type: primitive_type "i32"
      field_declaration
        name: field_identifier "w"
        type: primitive_type "i32"
```

**Key observations:**
- Plain `pub` makes `visibility_modifier` a leaf with text "pub"
- `pub(crate)` / `pub(super)` make `visibility_modifier` a parent with a `crate` or `super` child node
- No visibility → no `visibility_modifier` child at all

---

## generic type params with defaults

```rust
struct HashMap<K, V, S = DefaultHasher> {
    data: Vec<(K, V)>,
    hasher: S,
}
```

```
source_file
  struct_item
    name: type_identifier "HashMap"
    type_parameters: type_parameters
      type_parameter
        name: type_identifier "K"
      type_parameter
        name: type_identifier "V"
      type_parameter
        name: type_identifier "S"
        default_type: type_identifier "DefaultHasher"
    body: field_declaration_list
      field_declaration
        name: field_identifier "data"
        type: generic_type
          type: type_identifier "Vec"
          type_arguments: type_arguments
            tuple_type
              type_identifier "K"
              type_identifier "V"
      field_declaration
        name: field_identifier "hasher"
        type: type_identifier "S"
```

**Key observations:**
- Default type parameter → `default_type` field on `type_parameter`

---

## trait with associated type bounds

```rust
trait Container {
    type Item: Clone + Send;
    type Iter<'a>: Iterator<Item = &'a Self::Item> where Self: 'a;
}
```

```
source_file
  trait_item
    name: type_identifier "Container"
    body: declaration_list
      associated_type
        name: type_identifier "Item"
        bounds: trait_bounds
          type_identifier "Clone"
          type_identifier "Send"
      associated_type
        name: type_identifier "Iter"
        type_parameters: type_parameters
          lifetime_parameter
            name: lifetime
              identifier "a"
        bounds: trait_bounds
          generic_type
            type: type_identifier "Iterator"
            type_arguments: type_arguments
              type_binding
                name: type_identifier "Item"
                type: reference_type
                  lifetime
                    identifier "a"
                  type: scoped_type_identifier
                    path: identifier "Self"
                    name: type_identifier "Item"
        where_clause
          where_predicate
            left: type_identifier "Self"
            bounds: trait_bounds
              lifetime
                identifier "a"
```

**Key observations:**
- `associated_type` can have `bounds: trait_bounds`, `type_parameters`, and `where_clause`
- GATs (`type Iter<'a>`) add `type_parameters` to `associated_type`

---

## return position impl Trait

```rust
fn make_iter() -> impl Iterator<Item = i32> + Send {
    vec![1, 2, 3].into_iter()
}
```

```
source_file
  function_item
    name: identifier "make_iter"
    parameters: parameters "()"
    return_type: bounded_type
      abstract_type
        trait: generic_type
          type: type_identifier "Iterator"
          type_arguments: type_arguments
            type_binding
              name: type_identifier "Item"
              type: primitive_type "i32"
      type_identifier "Send"
    body: block
      call_expression
        function: field_expression
          value: macro_invocation
            macro: identifier "vec"
            token_tree
              integer_literal "1"
              integer_literal "2"
              integer_literal "3"
          field: field_identifier "into_iter"
        arguments: arguments "()"
```

**Key observations:**
- `impl Trait` → `abstract_type` with `trait` field
- `impl A + B` → `bounded_type` containing `abstract_type` (for `impl A`) + additional bounds
- This means the return type is `bounded_type` > [`abstract_type`, `type_identifier "Send"`]

---

## dyn Trait with lifetime

```rust
fn take(x: Box<dyn Display + Send + 'static>) {}
```

```
source_file
  function_item
    name: identifier "take"
    parameters: parameters
      parameter
        pattern: identifier "x"
        type: generic_type
          type: type_identifier "Box"
          type_arguments: type_arguments
            bounded_type
              bounded_type
                dynamic_type
                  trait: type_identifier "Display"
                type_identifier "Send"
              lifetime
                identifier "static"
    body: block "{}"
```

**Key observations:**
- `dyn Display` → `dynamic_type` with `trait` field
- `dyn A + B + 'c` → nested `bounded_type` (left-associated): `bounded_type(bounded_type(dynamic_type, Send), 'static)`
- This nesting means multiple bounds produce a LEFT-recursive `bounded_type` chain

---

## labeled block and labeled loops

```rust
fn f() {
    let x = 'block: {
        if true { break 'block 1; }
        2
    };
    'outer: loop {
        'inner: loop { break 'outer; }
    }
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters "()"
    body: block
      let_declaration
        pattern: identifier "x"
        value: block
          label
            identifier "block"
          expression_statement
            if_expression
              condition: boolean_literal "true"
              consequence: block
                expression_statement
                  break_expression
                    label
                      identifier "block"
                    integer_literal "1"
          integer_literal "2"
      expression_statement
        loop_expression
          label
            identifier "outer"
          body: block
            expression_statement
              loop_expression
                label
                  identifier "inner"
                body: block
                  expression_statement
                    break_expression
                      label
                        identifier "outer"
```

**Key observations:**
- Labeled blocks are plain `block` nodes with a `label` child
- `label` contains `identifier` (the `'` is anonymous)
- `break 'label value` → `break_expression` with `label` and value children
- Labels on loops appear as children of the loop expression, before `body`

---

## let-else statement

```rust
fn f(x: Option<i32>) {
    let Some(val) = x else { return; };
}
```

```
source_file
  function_item
    name: identifier "f"
    parameters: parameters
      parameter
        pattern: identifier "x"
        type: generic_type
          type: type_identifier "Option"
          type_arguments: type_arguments
            primitive_type "i32"
    body: block
      let_declaration
        pattern: tuple_struct_pattern
          type: identifier "Some"
          identifier "val"
        value: identifier "x"
        alternative: block
          expression_statement
            return_expression "return"
```

**Key observations:**
- `let ... else { ... }` uses the `alternative` field on `let_declaration`
- The diverging block is a regular `block`
- This mirrors `if_expression`'s `alternative: else_clause` pattern

---

## Summary of key node-kind distinctions

| Rust construct | tree-sitter node kind |
|---|---|
| `macro_rules! name { ... }` | `macro_definition` |
| `name!(...)` / `name![...]` / `name!{...}` | `macro_invocation` |
| struct (named fields) | `struct_item` > `field_declaration_list` |
| struct (tuple) | `struct_item` > `ordered_field_declaration_list` |
| struct (unit) | `struct_item` (no body) |
| enum variant (unit) | `enum_variant` (no body) |
| enum variant (tuple) | `enum_variant` > `ordered_field_declaration_list` |
| enum variant (struct) | `enum_variant` > `field_declaration_list` |
| trait method (no body) | `function_signature_item` |
| trait method (with body) | `function_item` |
| tail expression | bare child of `block` |
| expression statement | `expression_statement` > expr |
| `impl Type` | `impl_item` with `type` only |
| `impl Trait for Type` | `impl_item` with `trait` + `type` |
| `impl Trait` (in type) | `abstract_type` |
| `dyn Trait` | `dynamic_type` |
| `A + B` (type bounds) | `bounded_type` (left-recursive) |
| `pub` | `visibility_modifier` (leaf) |
| `pub(crate)` | `visibility_modifier` > `crate` |
| turbofish on method | `generic_function` |
| turbofish on path | `generic_type` in `scoped_identifier.path` |
| `async`/`unsafe`/`const` fn | `function_modifiers` |
| `let ... else { }` | `let_declaration` with `alternative` field |
