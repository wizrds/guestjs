use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Ident, Path, Token, Type,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream},
};

use crate::{attributes::HelperAttributes, guest::facade::GuestFacadeKind};

#[derive(Default, FromMeta)]
#[darling(default)]
struct MemberOptions {
    name: Option<String>,
}

impl MemberOptions {
    fn resolve(attributes: &mut Vec<Attribute>, ident: &Ident) -> syn::Result<String> {
        Ok(Self::from_list(
            &HelperAttributes::take(attributes)
                .map_err(|error| syn::Error::new(ident.span(), error.to_string()))?,
        )
        .map_err(|error| syn::Error::new(ident.span(), error.to_string()))?
        .name
        .unwrap_or_else(|| ident.unraw().to_string()))
    }
}

pub struct GuestParameter {
    ident: Ident,
    ty: Type,
}

impl GuestParameter {
    pub fn ident(&self) -> &Ident {
        &self.ident
    }

    pub fn ty(&self) -> &Type {
        &self.ty
    }
}

impl Parse for GuestParameter {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse()?;

        input.parse::<Token![:]>()?;

        Ok(Self { ident, ty: input.parse()? })
    }
}

pub enum GuestMember {
    Function {
        attributes: Vec<Attribute>,
        ident: Ident,
        guest_name: String,
        parameters: Vec<GuestParameter>,
        descriptor: Type,
    },
    Value {
        attributes: Vec<Attribute>,
        ident: Ident,
        guest_name: String,
        descriptor: Type,
    },
}

impl GuestMember {
    pub fn ident(&self) -> &Ident {
        match self {
            Self::Function { ident, .. } | Self::Value { ident, .. } => ident,
        }
    }

    pub fn render(
        &self,
        krate: &Path,
        receiver: &TokenStream,
        kind: &GuestFacadeKind,
    ) -> TokenStream {
        match self {
            Self::Function {
                attributes,
                ident,
                guest_name,
                parameters,
                descriptor,
            } => {
                let names = parameters
                    .iter()
                    .map(GuestParameter::ident);
                let types = parameters
                    .iter()
                    .map(GuestParameter::ty);
                let argument_types = parameters
                    .iter()
                    .map(GuestParameter::ty);
                let invocation = kind.invocation(
                    krate,
                    receiver,
                    guest_name,
                    descriptor,
                    &parameters
                        .iter()
                        .map(|parameter| {
                            let ident = parameter.ident();

                            quote!(#ident)
                        })
                        .collect::<Vec<_>>(),
                );

                quote! {
                    #(#attributes)*
                    pub fn #ident(&self, #(#names: #types),*)
                        -> ::core::result::Result<
                            <#descriptor as #krate::marshal::FromGuest<B>>::Owned,
                            #krate::errors::Error,
                        >
                    where
                        (#(#argument_types,)*): #krate::marshal::ToGuestArgs<B>,
                        #descriptor: #krate::marshal::FromGuest<B>,
                    {
                        #invocation
                    }
                }
            }
            Self::Value {
                attributes,
                ident,
                guest_name,
                descriptor,
            } => {
                quote! {
                    #(#attributes)*
                    pub fn #ident(&self)
                        -> ::core::result::Result<
                            <#descriptor as #krate::marshal::FromGuest<B>>::Owned,
                            #krate::errors::Error,
                        >
                    where
                        #descriptor: #krate::marshal::FromGuest<B>,
                    {
                        #krate::handle::protocol::ObjectProtocol::get::<
                            #descriptor,
                            _,
                        >(#receiver, #guest_name)
                    }
                }
            }
        }
    }
}

impl Parse for GuestMember {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;

        if input.peek(Token![fn]) {
            input.parse::<Token![fn]>()?;

            let ident = input.parse()?;
            let parameters;

            parenthesized!(parameters in input);

            let parameters = parameters
                .parse_terminated(GuestParameter::parse, Token![,])?
                .into_iter()
                .collect();

            input.parse::<Token![->]>()?;

            let descriptor = input.parse()?;

            input.parse::<Token![;]>()?;

            Ok(Self::Function {
                guest_name: MemberOptions::resolve(&mut attributes, &ident)?,
                attributes,
                ident,
                parameters,
                descriptor,
            })
        } else {
            let keyword = input.parse::<Ident>()?;

            if keyword != "value" {
                return Err(syn::Error::new(keyword.span(), "expected fn or value"));
            }

            let ident = input.parse()?;

            input.parse::<Token![:]>()?;

            let descriptor = input.parse()?;

            input.parse::<Token![;]>()?;

            Ok(Self::Value {
                guest_name: MemberOptions::resolve(&mut attributes, &ident)?,
                attributes,
                ident,
                descriptor,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{ImplItemFn, parse_quote, parse2};

    use crate::guest::{facade::GuestFacadeKind, member::GuestMember};

    #[test]
    fn emits_a_value_getter_through_the_public_protocol() {
        assert_eq!(
            parse2::<ImplItemFn>(
                parse2::<GuestMember>(quote!(value status: String;))
                    .unwrap()
                    .render(
                        &parse_quote!(crate),
                        &quote!(self.instance()),
                        &GuestFacadeKind::Class,
                    ),
            )
            .unwrap(),
            parse_quote! {
                pub fn status(&self) -> ::core::result::Result<
                    <String as crate::marshal::FromGuest<B>>::Owned,
                    crate::errors::Error,
                >
                where
                    String: crate::marshal::FromGuest<B>,
                {
                    crate::handle::protocol::ObjectProtocol::get::<
                        String,
                        _,
                    >(self.instance(), "status")
                }
            },
        );
    }
}
