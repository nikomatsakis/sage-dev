use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;
use synstructure::{AddBounds, decl_derive};

decl_derive!([FromImpls] => from_impls_derive);

/// Transforms an `async fn` into one whose body is `Box::pin(async move { ... }).await`.
/// This gives the function a concrete return type, enabling recursive async calls
/// without infinite type sizes.
#[proc_macro_attribute]
pub fn boxed_async_fn(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut item: ItemFn = syn::parse_macro_input!(input as ItemFn);

    if item.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&item.sig.fn_token, "expected an async function")
            .into_compile_error()
            .into();
    }

    let block = &item.block;
    item.block = syn::parse2(quote!({ Box::pin(async move #block).await })).unwrap();

    TokenStream::from(quote!(#item))
}

fn from_impls_derive(mut s: synstructure::Structure) -> proc_macro2::TokenStream {
    s.add_bounds(AddBounds::None);
    let mut impls = proc_macro2::TokenStream::new();

    for variant in s.variants() {
        let bindings = variant.bindings();
        if bindings.len() != 1 {
            continue;
        }

        let binding = &bindings[0];
        let field_ty = &binding.ast().ty;
        let construct = variant.construct(|_field, _idx| quote!(value));

        impls.extend(s.gen_impl(quote! {
            gen impl ::core::convert::From<#field_ty> for @Self {
                fn from(value: #field_ty) -> Self {
                    #construct
                }
            }
        }));
    }

    impls
}
