use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, GenericParam, Lifetime, parse_macro_input};

/// Derive `AllocStashData` for a `Copy` type with at most one lifetime `'db`.
#[proc_macro_derive(AllocStashData)]
pub fn derive_alloc_arena_data(input: TokenStream) -> TokenStream {
    derive_arena_data_impl(input)
}

/// Legacy alias — behaves identically to `AllocStashData`.
#[proc_macro_derive(InternStashData)]
pub fn derive_intern_arena_data(input: TokenStream) -> TokenStream {
    derive_arena_data_impl(input)
}

fn derive_arena_data_impl(input: TokenStream) -> TokenStream {
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

    if let Some(lt) = lifetimes.first()
        && lt.ident != "db"
    {
        return syn::Error::new_spanned(lt, "the lifetime parameter must be named 'db")
            .to_compile_error()
            .into();
    }

    let has_lifetime = !lifetimes.is_empty();

    // For static_type_id: use Name<'static> if generic, else just Name.
    let static_ty = if has_lifetime {
        quote! { #name<'static> }
    } else {
        quote! { #name }
    };

    let trait_name = quote! { ::sage_stash::AllocStashData };

    let ty = if has_lifetime {
        quote! { #name<'db> }
    } else {
        quote! { #name }
    };

    let stash_hash_body = generate_stash_hash_body(&input);

    let impl_generics = if has_lifetime {
        quote! { <'db> }
    } else {
        quote! {}
    };

    let stash_copy_body = generate_stash_copy_body(&input);

    quote! {
        unsafe impl<'db> ::sage_stash::StashData<'db> for #ty {
            fn static_type_id() -> ::core::any::TypeId {
                ::core::any::TypeId::of::<#static_ty>()
            }
        }

        impl<'db> #trait_name<'db> for #ty {}

        impl #impl_generics ::sage_stash::StashHash for #ty {
            fn stash_hash(
                &self,
                stash: &::sage_stash::Stash,
                hasher: &mut impl ::sage_stash::StashHasher,
            ) {
                #stash_hash_body
            }
        }

        impl #impl_generics ::sage_stash::StashCopy for #ty {
            fn stash_copy(
                &self,
                __stash_src: &::sage_stash::Stash,
                __stash_dst: &mut ::sage_stash::Stash,
            ) -> Self {
                #stash_copy_body
            }
        }
    }
    .into()
}

fn generate_stash_hash_body(input: &DeriveInput) -> proc_macro2::TokenStream {
    match &input.data {
        syn::Data::Struct(data) => generate_stash_hash_fields(&data.fields),
        syn::Data::Enum(data) => {
            let arms: Vec<_> = data
                .variants
                .iter()
                .enumerate()
                .map(|(idx, variant)| {
                    let variant_ident = &variant.ident;
                    let idx = idx as u32;
                    match &variant.fields {
                        syn::Fields::Named(fields) => {
                            let field_names: Vec<_> =
                                fields.named.iter().map(|f| &f.ident).collect();
                            let hash_stmts: Vec<_> = fields
                                .named
                                .iter()
                                .map(|f| {
                                    let fname = &f.ident;
                                    quote! { ::sage_stash::StashHash::stash_hash(#fname, stash, hasher); }
                                })
                                .collect();
                            quote! {
                                Self::#variant_ident { #(#field_names),* } => {
                                    ::core::hash::Hash::hash(&#idx, hasher);
                                    #(#hash_stmts)*
                                }
                            }
                        }
                        syn::Fields::Unnamed(fields) => {
                            let bindings: Vec<_> = (0..fields.unnamed.len())
                                .map(|i| {
                                    syn::Ident::new(
                                        &format!("f{i}"),
                                        proc_macro2::Span::call_site(),
                                    )
                                })
                                .collect();
                            let hash_stmts: Vec<_> = bindings
                                .iter()
                                .map(|binding| {
                                    quote! { ::sage_stash::StashHash::stash_hash(#binding, stash, hasher); }
                                })
                                .collect();
                            quote! {
                                Self::#variant_ident(#(#bindings),*) => {
                                    ::core::hash::Hash::hash(&#idx, hasher);
                                    #(#hash_stmts)*
                                }
                            }
                        }
                        syn::Fields::Unit => {
                            quote! {
                                Self::#variant_ident => {
                                    ::core::hash::Hash::hash(&#idx, hasher);
                                }
                            }
                        }
                    }
                })
                .collect();
            quote! {
                match self {
                    #(#arms)*
                }
            }
        }
        syn::Data::Union(_) => {
            syn::Error::new_spanned(&input.ident, "StashHash cannot be derived for unions")
                .to_compile_error()
        }
    }
}

fn generate_stash_hash_fields(fields: &syn::Fields) -> proc_macro2::TokenStream {
    match fields {
        syn::Fields::Named(named) => {
            let stmts: Vec<_> = named
                .named
                .iter()
                .map(|f| {
                    let fname = &f.ident;
                    quote! { ::sage_stash::StashHash::stash_hash(&self.#fname, stash, hasher); }
                })
                .collect();
            quote! { #(#stmts)* }
        }
        syn::Fields::Unnamed(unnamed) => {
            let stmts: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| {
                    let idx = syn::Index::from(i);
                    quote! { ::sage_stash::StashHash::stash_hash(&self.#idx, stash, hasher); }
                })
                .collect();
            quote! { #(#stmts)* }
        }
        syn::Fields::Unit => quote! {},
    }
}

// ---------------------------------------------------------------------------
// StashCopy code generation
// ---------------------------------------------------------------------------

fn generate_stash_copy_body(input: &DeriveInput) -> proc_macro2::TokenStream {
    match &input.data {
        syn::Data::Struct(data) => generate_stash_copy_struct_fields(&data.fields),
        syn::Data::Enum(data) => {
            let arms: Vec<_> = data
                .variants
                .iter()
                .map(|variant| {
                    let variant_ident = &variant.ident;
                    match &variant.fields {
                        syn::Fields::Named(fields) => {
                            let field_names: Vec<_> =
                                fields.named.iter().map(|f| &f.ident).collect();
                            let copy_exprs: Vec<_> = field_names
                                .iter()
                                .map(|fname| {
                                    quote! { #fname: ::sage_stash::StashCopy::stash_copy(#fname, __stash_src, __stash_dst) }
                                })
                                .collect();
                            quote! {
                                Self::#variant_ident { #(#field_names),* } => {
                                    Self::#variant_ident { #(#copy_exprs),* }
                                }
                            }
                        }
                        syn::Fields::Unnamed(fields) => {
                            let bindings: Vec<_> = (0..fields.unnamed.len())
                                .map(|i| {
                                    syn::Ident::new(
                                        &format!("f{i}"),
                                        proc_macro2::Span::call_site(),
                                    )
                                })
                                .collect();
                            let copy_exprs: Vec<_> = bindings
                                .iter()
                                .map(|binding| {
                                    quote! { ::sage_stash::StashCopy::stash_copy(#binding, __stash_src, __stash_dst) }
                                })
                                .collect();
                            quote! {
                                Self::#variant_ident(#(#bindings),*) => {
                                    Self::#variant_ident(#(#copy_exprs),*)
                                }
                            }
                        }
                        syn::Fields::Unit => {
                            quote! {
                                Self::#variant_ident => Self::#variant_ident,
                            }
                        }
                    }
                })
                .collect();
            quote! {
                match self {
                    #(#arms)*
                }
            }
        }
        syn::Data::Union(_) => {
            syn::Error::new_spanned(&input.ident, "StashCopy cannot be derived for unions")
                .to_compile_error()
        }
    }
}

fn generate_stash_copy_struct_fields(fields: &syn::Fields) -> proc_macro2::TokenStream {
    match fields {
        syn::Fields::Named(named) => {
            let field_copies: Vec<_> = named
                .named
                .iter()
                .map(|f| {
                    let fname = &f.ident;
                    quote! { #fname: ::sage_stash::StashCopy::stash_copy(&self.#fname, __stash_src, __stash_dst) }
                })
                .collect();
            quote! { Self { #(#field_copies),* } }
        }
        syn::Fields::Unnamed(unnamed) => {
            let field_copies: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| {
                    let idx = syn::Index::from(i);
                    quote! { ::sage_stash::StashCopy::stash_copy(&self.#idx, __stash_src, __stash_dst) }
                })
                .collect();
            quote! { Self(#(#field_copies),*) }
        }
        syn::Fields::Unit => quote! { Self },
    }
}
