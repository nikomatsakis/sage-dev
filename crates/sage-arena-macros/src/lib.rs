use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, GenericParam, Lifetime, parse_macro_input};

/// Derive `AllocArenaData` for a `Copy` type with at most one lifetime `'db`.
#[proc_macro_derive(AllocArenaData)]
pub fn derive_alloc_arena_data(input: TokenStream) -> TokenStream {
    derive_arena_data_impl(input, false)
}

/// Derive `InternArenaData` for a `Copy + Hash + Eq` type with at most one lifetime `'db`.
#[proc_macro_derive(InternArenaData)]
pub fn derive_intern_arena_data(input: TokenStream) -> TokenStream {
    derive_arena_data_impl(input, true)
}

fn derive_arena_data_impl(input: TokenStream, intern: bool) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Validate: no type params, at most one lifetime named 'db.
    let lifetimes: Vec<&Lifetime> = input
        .generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Lifetime(lt) => Some(&lt.lifetime),
            GenericParam::Type(_) | GenericParam::Const(_) => None,
        })
        .collect();

    for p in &input.generics.params {
        if matches!(p, GenericParam::Type(_) | GenericParam::Const(_)) {
            return syn::Error::new_spanned(
                p,
                "arena data types must not have type or const parameters",
            )
            .to_compile_error()
            .into();
        }
    }

    if lifetimes.len() > 1 {
        return syn::Error::new_spanned(
            &input.generics,
            "arena data types must have at most one lifetime parameter",
        )
        .to_compile_error()
        .into();
    }

    if let Some(lt) = lifetimes.first() {
        if lt.ident != "db" {
            return syn::Error::new_spanned(lt, "the lifetime parameter must be named 'db")
                .to_compile_error()
                .into();
        }
    }

    let has_lifetime = !lifetimes.is_empty();

    // For static_type_id: use Name<'static> if generic, else just Name.
    let static_ty = if has_lifetime {
        quote! { #name<'static> }
    } else {
        quote! { #name }
    };

    let trait_name = if intern {
        quote! { ::sage_arena::InternArenaData }
    } else {
        quote! { ::sage_arena::AllocArenaData }
    };

    let ty = if has_lifetime {
        quote! { #name<'db> }
    } else {
        quote! { #name }
    };

    quote! {
        unsafe impl<'db> ::sage_arena::ArenaData<'db> for #ty {
            fn static_type_id() -> ::core::any::TypeId {
                ::core::any::TypeId::of::<#static_ty>()
            }
        }

        impl<'db> #trait_name<'db> for #ty {}
    }
    .into()
}
