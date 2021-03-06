extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Fields, FnArg, Ident, ImplItem, ItemImpl, ItemStruct, ItemTrait, Pat,
    ReturnType, TraitItem, Type,
};

#[proc_macro_attribute]
pub fn interface(attr: TokenStream, defn: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(defn as ItemTrait);
    let user_error = parse_macro_input!(attr as Type);

    let mut client_functions = quote! {};
    let mut dispatcher_cases = quote! {};

    for mut item in &mut input.items {
        match &mut item {
            TraitItem::Method(ref mut method) => {
                let signature = &mut method.sig;
                let original_result = &signature.output;
                match original_result {
                    ReturnType::Type(_, original_result) => {
                        let replacement = TokenStream::from(
                            quote! { -> ::bearings::Result<#original_result, #user_error> },
                        );
                        signature.output = parse_macro_input!(replacement as ReturnType);
                    }
                    _ => {
                        panic!("can't handle a function with default return type in a class");
                    }
                }

                let mut arguments = quote! {};
                let mut argument_tuple = quote! {};
                let mut argument_expansion = quote! {};

                let mut i: u32 = 0;

                for arg in &signature.inputs {
                    match arg {
                        FnArg::Receiver(_) => (),
                        FnArg::Typed(arg) => match &*arg.pat {
                            Pat::Ident(name) => {
                                arguments.extend(quote! {
                                    &#name,
                                });

                                let ty = &arg.ty;
                                argument_tuple.extend(quote! {
                                    #ty,
                                });

                                let idx = syn::Index::from(i as usize);
                                argument_expansion.extend(quote! {
                                    arguments.#idx,
                                });
                                i += 1;
                            }
                            _ => {
                                panic!("only an identifier is allowed as a name of a class method argument");
                            }
                        },
                    }
                }

                let name = &signature.ident;
                let return_type = match &signature.output {
                    ReturnType::Default => {
                        panic!("can't handle a method with a default return type");
                    }
                    ReturnType::Type(_, ty) => ty,
                };

                client_functions.extend(quote! {
                    #signature {
                        let id = {
                            let mut id_guard = self.state.id.lock().await;
                            let id = *id_guard;
                            *id_guard += 1;
                            id
                        };

                        let call = ::serde_json::to_string(&::bearings::Message::<_, #user_error>::Call(::bearings::FunctionCall{
                            id: id,
                            uuid: self.uuid.clone(),
                            member: self.member.to_string(),
                            method: stringify!(#name).to_string(),
                            arguments: (#arguments),
                        }))?;

                        println!("{}", call);

                        {
                            let mut map = self.state.awaiters.lock().await;
                            map.insert(id, ::tokio::sync::Mutex::from(::bearings::Awaiter::Empty));
                            println!("{:?}", map);
                        }

                        let mut w = self.state.w.lock().await;
                        use ::tokio::io::AsyncWriteExt;
                        w.write_all(format!("{}\0", call).as_bytes()).await?;
                        w.flush().await?;

                        ::bearings::ReplyFuture::<
                            <#return_type as ::std::iter::IntoIterator>::Item,
                            T,
                            #user_error
                        >::new(self.state.clone(), id).await
                    }
                });

                dispatcher_cases.extend(quote! {
                    stringify!(#name) => {
                        let arguments: (#argument_tuple) = ::serde_json::from_value(call.arguments)?;
                        Ok(::bearings::Message::<(), #user_error>::Return(
                            ::bearings::ReturnValue{
                                id: call.id,
                                result: ::serde_json::value::Value::from({
                                    let mut result = object.lock().await;
                                    let result = result.#name(#argument_expansion);
                                    result.await?
                                })
                            }
                        ))
                    }
                });
            }
            _ => {
                panic!("only methods are allowed inside a class trait");
            }
        }
    }

    let name = &input.ident;
    let error_name = syn::Ident::new(&format!("{}Error", name), syn::export::Span::call_site());
    let client_name = syn::Ident::new(&format!("{}Client", name), syn::export::Span::call_site());
    let dispatcher_name = syn::Ident::new(
        &format!("{}Dispatcher", name),
        syn::export::Span::call_site(),
    );

    let expanded = quote! {
        #[::bearings::async_trait]
        #input

        struct #dispatcher_name {
        }

        impl #dispatcher_name {
            async fn invoke_method<'a>(
                object: &::tokio::sync::Mutex<Box<dyn #name + Send + 'a>>,
                call: ::bearings::FunctionCall<serde_json::value::Value>,
            ) -> ::bearings::Result<::bearings::Message<(), #user_error>, #user_error> {
                match &call.method[..] {
                    #dispatcher_cases
                    _ => Err(::bearings::Error::UnknownMethod(call.member, call.method))
                }
            }
        }

        struct #client_name<T: Send + ::tokio::io::AsyncRead + ::tokio::io::AsyncWrite> {
            uuid: ::uuid::Uuid,
            member: &'static str,
            state: ::bearings::StatePtr<T, #user_error>,
        }

        #[::bearings::async_trait]
        impl<T: Send + Unpin + ::tokio::io::AsyncRead + ::tokio::io::AsyncWrite> #name for #client_name<T> {
            #client_functions
        }

        type #error_name = #user_error;
    };

    println!("{}", expanded);

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn class(attr: TokenStream, defn: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(defn as ItemImpl);
    let user_error = parse_macro_input!(attr as Type);

    for item in &mut input.items {
        match item {
            ImplItem::Method(ref mut method) => {
                let signature = &mut method.sig;
                let original_result = &signature.output;
                match original_result {
                    ReturnType::Type(_, original_result) => {
                        let replacement = TokenStream::from(
                            quote! { -> ::bearings::Result<#original_result, #user_error> },
                        );
                        signature.output = parse_macro_input!(replacement as ReturnType);
                    }
                    _ => {
                        panic!("can't handle a function with default return type in a class");
                    }
                }
            }

            _ => panic!("unsupported class element"),
        }
    }

    let expanded = quote! {
        #[::bearings::async_trait]
        #input
    };

    println!("{}", expanded);

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn object(attr: TokenStream, defn: TokenStream) -> TokenStream {
    let input = parse_macro_input!(defn as ItemStruct);
    let user_error = parse_macro_input!(attr as Type);

    let mut fields = quote!();
    let mut parameters = quote!();
    let mut arguments = quote!();
    let mut init = quote!();
    let mut client_init = quote!();
    let mut member_dispatch = quote!();

    let mut i: u32 = 0;

    match input.fields {
        Fields::Named(ref named) => {
            for field in named.named.iter() {
                let name = field.ident.as_ref().unwrap();
                let ty = &field.ty;

                fields.extend(quote! {
                    #name: ::tokio::sync::Mutex<Box<dyn #ty + Send + 'a>>,
                });

                let param_type =
                    syn::Ident::new(&format!("T{}", i), syn::export::Span::call_site());
                i += 1;

                parameters.extend(quote! {
                    #param_type: #ty + Send + 'a,
                });

                arguments.extend(quote! {
                    #name: #param_type,
                });

                init.extend(quote! {
                    #name: ::tokio::sync::Mutex::from(Box::from(#name) as Box<dyn #ty + Send + 'a>),
                });

                let (client_type, dispatcher_type) = match ty {
                    Type::Path(path) => {
                        let mut client = path.clone();
                        let mut dispatcher = path.clone();

                        let mut last = client.path.segments.pop().unwrap().into_value();
                        last.ident =
                            Ident::new(&format!("{}Client", last.ident), last.ident.span());
                        client.path.segments.push_value(last);

                        let mut last = dispatcher.path.segments.pop().unwrap().into_value();
                        last.ident =
                            Ident::new(&format!("{}Dispatcher", last.ident), last.ident.span());
                        dispatcher.path.segments.push_value(last);

                        (client, dispatcher)
                    }
                    _ => {
                        panic!("the type of a field of an object structure must be a previously defined class");
                    }
                };

                client_init.extend(quote! {
                    #name: ::tokio::sync::Mutex::from(Box::from(#client_type {
                        uuid: uuid.clone(),
                        member: stringify!(#name),
                        state: state.clone()
                    }) as Box<dyn #ty + Send + 'a>),
                });

                member_dispatch.extend(quote! {
                    stringify!(#name) => #dispatcher_type::invoke_method(&self.#name, call).await,
                });
            }
        }

        _ => unimplemented!(),
    }

    let name = &input.ident;
    let expanded = quote! {
        struct #name<'a> {
            __: std::marker::PhantomData<&'a ()>,

            #fields
        }

        impl<'a> #name<'a> {
            pub fn new<#parameters>(#arguments) -> Self {
                Self{
                    __: <_>::default(),
                    #init
                }
            }

            fn uuid() -> ::uuid::Uuid {
                ::uuid::Uuid::new_v5(&::uuid::Uuid::nil(), stringify!(#name).as_bytes())
            }
        }

        #[::bearings::async_trait]
        impl<'a> ::bearings::Object<#user_error> for #name<'a> {
            fn uuid() -> ::uuid::Uuid {
                Self::uuid()
            }

            async fn invoke(
                &self,
                call: ::bearings::FunctionCall<::serde_json::value::Value>,
            ) -> ::bearings::Result<::bearings::Message<(), #user_error>, #user_error> {
                assert_eq!(Self::uuid(), call.uuid);

                match &call.member[..] {
                    #member_dispatch
                    _ => Err(::bearings::Error::UnknownMember(call.member))
                }
            }
        }

        impl<'a> ::bearings::ObjectClient<'a, #user_error> for #name<'a> {
            fn build<T: 'a + Send + Unpin + ::tokio::io::AsyncRead + ::tokio::io::AsyncWrite>(
                state: ::bearings::StatePtr<T, #user_error>
            ) -> Self {
                let uuid = Self::uuid();
                Self {
                    __: <_>::default(),
                    #client_init
                }
            }
        }

        unsafe impl Sync for #name<'_> {}
    };

    println!("{}", expanded);

    TokenStream::from(expanded)
}
