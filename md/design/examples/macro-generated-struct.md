# Macro-generated struct

Now let a macro create the struct used by ordinary code:

```rust
mod shapes {
    macro_rules! define_circle {
        () => {
            pub struct Circle { pub radius: f64 }
        };
    }

    define_circle!();
}

fn radius(circle: shapes::Circle) -> f64 {
    circle.radius
}
```

The key property is that signature and body checking do not need a second path
for macro output. Expansion produces ordinary item symbols; later resolution
consumes the expanded module item list.

## The expanded-module query

Local modules expose their expanded items through a Salsa tracked query:

```{anchor}
example_expanded_module_query
```

The query begins with parsed, unexpanded `LocalModItemSym` values. Ordinary
items proceed directly to attribute/derive processing, while a bang-macro
invocation is expanded recursively:

```{anchor}
example_expand_items
```

For `define_circle!()`, the resolver finds the macro in the macro namespace,
`parse_output` turns its output back into unexpanded item symbols, and the same
function processes those items:

```{anchor}
example_expand_macro
```

The `Circle` symbol therefore enters the final `Vec<Symbol>` exactly where a
source-written struct would. Nested expansion uses the same path. Recursive
access to the module query is handled by its declared Salsa cycle initial
value and fixed-point iteration.

## Resolution after expansion

When the signature query for `radius` resolves `shapes::Circle`, module lookup
asks `ModSymbol::expanded_module_items(db)` and searches the resulting symbols.
It finds the generated `Circle`, after which type lowering produces
`Ty::Adt(Circle, [])` and field lookup follows the path from the preceding
example.

The checker does not retain a special "came from this macro" type. Expansion
provenance remains available through spans and macro invocation symbols, while
the semantic pipeline operates on the generated item symbol.

The complete phase contract, including same-module fixed-point resolution and
terminal incompleteness, is in [Module and Macro
Expansion](../pipeline/module-expansion.md).
