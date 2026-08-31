use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, parse_macro_input};

// ANCHOR: semantic_inspector_reflect_derive
#[proc_macro_derive(Reflect)]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_reflect(input).into()
}

fn expand_reflect(input: DeriveInput) -> proc_macro2::TokenStream {
    let name = input.ident;
    let mut generics = input.generics;
    let type_generics = generics.clone();
    let reflect_lifetime = generics
        .lifetimes()
        .next()
        .map(|parameter| parameter.lifetime.clone())
        .unwrap_or_else(|| syn::parse_quote!('__reflect));
    if generics.lifetimes().next().is_none() {
        generics.params.insert(0, syn::parse_quote!('__reflect));
    }
    for parameter in generics.type_params_mut() {
        parameter
            .bounds
            .push(syn::parse_quote!(::sage_reflect::Reflect<#reflect_lifetime>));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = type_generics.split_for_impl();

    let body = match input.data {
        Data::Struct(data) => struct_body(&name, data.fields),
        Data::Enum(data) => {
            let arms = data.variants.into_iter().map(|variant| {
                let variant_name = variant.ident;
                match variant.fields {
                    Fields::Named(fields) => {
                        let names: Vec<_> = fields
                            .named
                            .iter()
                            .map(|field| field.ident.as_ref().unwrap())
                            .collect();
                        let values = names.iter().map(|field| {
                            let label = field.to_string();
                            quote! {
                                ::sage_reflect::ValueField::new(
                                    #label,
                                    ::sage_reflect::Reflect::reflect(#field, context, stash),
                                )
                            }
                        });
                        quote! {
                            Self::#variant_name { #(#names),* } => {
                                ::sage_reflect::ValueNode::variant(
                                    stringify!(#name),
                                    stringify!(#variant_name),
                                    vec![#(#values),*],
                                )
                            }
                        }
                    }
                    Fields::Unnamed(fields) => {
                        let names: Vec<_> = (0..fields.unnamed.len())
                            .map(|index| format_ident!("field_{index}"))
                            .collect();
                        let values = names.iter().enumerate().map(|(index, field)| {
                            let label = index.to_string();
                            quote! {
                                ::sage_reflect::ValueField::new(
                                    #label,
                                    ::sage_reflect::Reflect::reflect(#field, context, stash),
                                )
                            }
                        });
                        quote! {
                            Self::#variant_name(#(#names),*) => {
                                ::sage_reflect::ValueNode::variant(
                                    stringify!(#name),
                                    stringify!(#variant_name),
                                    vec![#(#values),*],
                                )
                            }
                        }
                    }
                    Fields::Unit => quote! {
                        Self::#variant_name => ::sage_reflect::ValueNode::variant(
                            stringify!(#name),
                            stringify!(#variant_name),
                            vec![],
                        )
                    },
                }
            });
            quote! { match self { #(#arms),* } }
        }
        Data::Union(_) => quote! { compile_error!("Reflect cannot be derived for unions") },
    };

    quote! {
        impl #impl_generics ::sage_reflect::Reflect<#reflect_lifetime> for #name #ty_generics #where_clause {
            fn reflect(
                &self,
                context: &mut ::sage_reflect::ReflectionContext<'_>,
                stash: ::core::option::Option<&::sage_stash::Stash>,
            ) -> ::sage_reflect::ValueNode {
                context.reflect_node(stringify!(#name), |context| #body)
            }
        }
    }
}

fn struct_body(name: &syn::Ident, fields: Fields) -> proc_macro2::TokenStream {
    match fields {
        Fields::Named(fields) => {
            let values = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let label = field_name.to_string();
                quote! {
                    ::sage_reflect::ValueField::new(
                        #label,
                        ::sage_reflect::Reflect::reflect(&self.#field_name, context, stash),
                    )
                }
            });
            quote! {
                ::sage_reflect::ValueNode::record(stringify!(#name), vec![#(#values),*])
            }
        }
        Fields::Unnamed(fields) => {
            let values = fields.unnamed.iter().enumerate().map(|(index, _)| {
                let field_index = syn::Index::from(index);
                let label = index.to_string();
                quote! {
                    ::sage_reflect::ValueField::new(
                        #label,
                        ::sage_reflect::Reflect::reflect(&self.#field_index, context, stash),
                    )
                }
            });
            quote! {
                ::sage_reflect::ValueNode::record(stringify!(#name), vec![#(#values),*])
            }
        }
        Fields::Unit => {
            quote! { ::sage_reflect::ValueNode::record(stringify!(#name), vec![]) }
        }
    }
}
// ANCHOR_END: semantic_inspector_reflect_derive
