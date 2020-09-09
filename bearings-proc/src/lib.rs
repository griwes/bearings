extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Fields, ItemStruct};

#[proc_macro_attribute]
pub fn class(_attr: TokenStream, defn: TokenStream) -> TokenStream {
    let input = proc_macro2::TokenStream::from(defn);

    let expanded = quote! {
        #[::bearings::async_trait]
        #input
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn method(_attr: TokenStream, defn: TokenStream) -> TokenStream {
    defn
}

#[proc_macro_attribute]
pub fn object(_attr: TokenStream, defn: TokenStream) -> TokenStream {
    let input = parse_macro_input!(defn as ItemStruct);

    let mut arguments = quote!();
    let mut init = quote!();
    match input.fields {
        Fields::Named(ref named) => {
            for field in named.named.iter() {
                let name = field.ident.as_ref().unwrap();
                let ty = &field.ty;

                arguments.extend(quote! {
                    #name: #ty,
                });

                init.extend(quote![#name: #name,]);
            }
        }

        _ => unimplemented!(),
    }

    let name = &input.ident;
    let expanded = quote! {
        #input

        impl #name {
            pub fn new(#arguments) -> Self {
                Self{ #init }
            }
        }

        impl ::bearings::Object for #name {
            fn uuid(&self) -> ::uuid::Uuid {
                ::uuid::Uuid::new_v5(&::uuid::Uuid::nil(), stringify!(#name).as_bytes())
            }
        }

        impl ::bearings::ObjectClient for #name {
            fn build<T>(state: ::bearings::StatePtr<T>) -> Self {
                unimplemented!();
            }
        }
    };

    TokenStream::from(expanded)
}

//let client_name = syn::Ident::new(&format!("{}Client", name), syn::export::Span::call_site());
