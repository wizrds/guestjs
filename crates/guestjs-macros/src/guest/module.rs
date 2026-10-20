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

pub struct GuestModuleMacro {
    syntax: GuestFacadeSyntax,
}

impl GuestModuleMacro {
    pub fn new(tokens: TokenStream) -> Result<Self, MacroError> {
        Ok(Self {
            syntax: GuestFacadeSyntax::new(tokens, GuestFacadeKind::Module)?,
        })
    }

    pub fn expand(self) -> TokenStream {
        let (attributes, visibility, name, krate, _, members) =
            self.syntax.into_parts();
        let conditions = HelperAttributes::conditions(&attributes);
        let inner = quote!(#krate::handle::module::Module<B>);
        let conversion_bounds = quote!(
            B: #krate::backend::values::BackendValues,
        );
        let payload_bound = TokenStream::new();
        let methods = members.iter().map(|member| {
            member.render(
                &krate,
                &quote!(self.module()),
                &GuestFacadeKind::Module,
            )
        });

        quote! {
            #(#attributes)*
            #visibility struct #name<B: #krate::backend::Backend> {
                module: #inner,
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #name<B> {
                pub fn new(module: #inner) -> Self {
                    Self { module }
                }

                pub fn module(&self) -> &#inner {
                    &self.module
                }

                pub fn into_module(self) -> #inner {
                    self.module
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::values::BackendValues> #name<B> {
                #(#methods)*
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> ::core::clone::Clone
                for #name<B>
            {
                fn clone(&self) -> Self {
                    Self::new(self.module().clone())
                }
            }

            #(#conditions)*
            impl<B: #krate::backend::Backend> #krate::handle::HasHandle<B>
                for #name<B>
            {
                fn handle(&self) -> &#krate::handle::base::Handle<B> {
                    #krate::handle::HasHandle::handle(self.module())
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
                        self.into_module(),
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

    use crate::guest::module::GuestModuleMacro;

    #[test]
    fn emits_the_complete_owned_facade() {
        assert_eq!(
            parse2::<File>(
                GuestModuleMacro::new(quote!(
                    #[guestjs(crate_path = crate)]
                    pub module Client {
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
            7,
        );
    }
}
