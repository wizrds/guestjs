use proc_macro2::TokenStream;
use quote::quote;

use crate::{
    diagnostics::MacroError,
    guest::facade::{
        GuestFacadeKind,
        GuestFacadeSyntax,
    },
    attributes::HelperAttributes,
};

pub struct GuestClassMacro {
    syntax: GuestFacadeSyntax,
}

impl GuestClassMacro {
    pub fn new(tokens: TokenStream) -> Result<Self, MacroError> {
        Ok(Self {
            syntax: GuestFacadeSyntax::new(tokens, GuestFacadeKind::Class)?,
        })
    }

    pub fn expand(self) -> TokenStream {
        let (attributes, visibility, name, krate, payload, members) =
            self.syntax.into_parts();
        let conditions = HelperAttributes::conditions(&attributes);
        let inner = match &payload {
            Some(payload) => quote!(#krate::handle::class::Instance<B, #payload>),
            None => quote!(#krate::handle::class::Instance<B>),
        };
        let conversion_bounds = if payload.is_some() {
            quote!(B: #krate::backend::classes::BackendClasses,)
        } else {
            quote!(B: #krate::backend::values::BackendValues,)
        };
        let payload_bound = payload.as_ref().map(|payload| {
            quote!(#payload: #krate::handle::class::Payload,)
        });
        let methods = members.iter().map(|member| {
            member.render(
                &krate,
                &quote!(self.instance()),
                &GuestFacadeKind::Class,
            )
        });

        quote! {
            #(#attributes)*
            #visibility struct #name<B: #krate::backend::Backend> {
                instance: #inner,
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #name<B> {
                fn new(instance: #inner) -> Self {
                    Self { instance }
                }

                pub fn instance(&self) -> &#inner {
                    &self.instance
                }

                pub fn into_instance(self) -> #inner {
                    self.instance
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::values::BackendValues> #name<B> {
                #(#methods)*
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> ::core::convert::AsRef<#inner>
                for #name<B>
            {
                fn as_ref(&self) -> &#inner {
                    self.instance()
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> ::core::convert::From<#inner>
                for #name<B>
            {
                fn from(instance: #inner) -> Self {
                    Self::new(instance)
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> ::core::convert::Into<#inner>
                for #name<B>
            {
                fn into(self) -> #inner {
                    self.into_instance()
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> ::core::clone::Clone
                for #name<B>
            {
                fn clone(&self) -> Self {
                    Self::new(self.instance().clone())
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #krate::handle::HasHandle<B>
                for #name<B>
            {
                fn handle(&self) -> &#krate::handle::base::Handle<B> {
                    #krate::handle::HasHandle::handle(self.instance())
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #krate::marshal::FromGuest<B>
                for #name<B>
            where
                #conversion_bounds
                #payload_bound
            {
                type Owned = Self;

                fn from_guest<'js>(
                    enter: &#krate::enter::Enter<'js, B>,
                    value: #krate::backend::Val<'js, B>,
                ) -> ::core::result::Result<Self::Owned, #krate::errors::Error> {
                    <#inner as #krate::marshal::FromGuest<B>>::from_guest(
                        enter,
                        value,
                    ).map(Self::new)
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #krate::marshal::ToGuest<B>
                for #name<B>
            {
                fn to_guest<'js>(
                    self,
                    enter: &#krate::enter::Enter<'js, B>,
                ) -> ::core::result::Result<
                    #krate::backend::Val<'js, B>,
                    #krate::errors::Error,
                > {
                    #krate::marshal::ToGuest::to_guest(
                        self.into_instance(),
                        enter,
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{
        parse2,
        File,
    };

    use crate::guest::class::GuestClassMacro;

    #[test]
    fn emits_the_complete_owned_facade() {
        assert_eq!(
            parse2::<File>(
                GuestClassMacro::new(quote!(
                    #[guestjs(crate_path = crate)]
                    pub class Client {
                        #[guestjs(name = "sendRequest")]
                        fn send(request: String) -> String;

                        value status: String;
                    }
                ))
                .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
                .expand(),
            )
            .unwrap()
            .items.len(),
            10,
        );
    }
}
