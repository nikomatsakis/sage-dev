use quote::quote;
use synstructure::{AddBounds, decl_derive};

decl_derive!([FromImpls] => from_impls_derive);

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
