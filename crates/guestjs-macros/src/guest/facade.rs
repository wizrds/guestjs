use darling::FromMeta;
use proc_macro2::{
    Span,
    TokenStream,
};
use quote::quote;
use syn::{
    braced,
    parse::{
        ParseStream,
        Parser,
    },
    Attribute,
    Ident,
    Path,
    Type,
    Visibility,
};

use crate::{
    attributes::HelperAttributes,
    guest::member::GuestMember,
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct FacadeOptions {
    crate_path: Option<Path>,
    payload: Option<Path>,
}

impl FacadeOptions {
    fn resolve(
        attributes: &mut Vec<Attribute>,
        kind: GuestFacadeKind,
    ) -> syn::Result<(Path, Option<Path>)> {
        let options = Self::from_list(
            &HelperAttributes::take(attributes).map_err(|error| {
                syn::Error::new(Span::call_site(), error.to_string())
            })?,
        ).map_err(|error| {
            syn::Error::new(Span::call_site(), error.to_string())
        })?;

        if kind == GuestFacadeKind::Module && options.payload.is_some() {
            return Err(syn::Error::new(
                Span::call_site(),
                "payload is only valid on a class facade",
            ));
        }

        Ok((
            CratePath::new(options.crate_path).resolve()?,
            options.payload,
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GuestFacadeKind {
    Class,
    Module,
}

impl GuestFacadeKind {
    pub fn invocation(
        &self,
        krate: &Path,
        receiver: &TokenStream,
        guest_name: &str,
        descriptor: &Type,
        arguments: &[TokenStream],
    ) -> TokenStream {
        match self {
            Self::Class => quote! {
                #krate::handle::protocol::ObjectProtocol::call_method::<
                    #descriptor, _, _,
                >(
                    #receiver,
                    #guest_name,
                    (#(#arguments,)*),
                )
            },
            Self::Module => quote! {
                #krate::handle::protocol::CallableProtocol::call::<
                    #descriptor, _, _,
                >(
                    &#krate::handle::protocol::ObjectProtocol::get::<
                        #krate::handle::function::Function<B>, _,
                    >(#receiver, #guest_name)?,
                    (),
                    (#(#arguments,)*),
                )
            },
        }
    }
}

pub struct GuestFacadeSyntax {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    krate: Path,
    payload: Option<Path>,
    members: Vec<GuestMember>,
}

impl GuestFacadeSyntax {
    pub fn new(
        tokens: TokenStream,
        kind: GuestFacadeKind,
    ) -> syn::Result<Self> {
        (|input: ParseStream| Self::parse(input, kind)).parse2(tokens)
    }

    fn parse(
        input: ParseStream,
        kind: GuestFacadeKind,
    ) -> syn::Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;
        let visibility = input.parse()?;
        let keyword = input.parse::<Ident>()?;

        if keyword != match kind {
            GuestFacadeKind::Class => "class",
            GuestFacadeKind::Module => "module",
        } {
            return Err(syn::Error::new(keyword.span(), "unexpected facade kind"));
        }

        let name = input.parse()?;
        let contents;

        braced!(contents in input);

        let mut members = Vec::new();
        let mut names = match kind {
            GuestFacadeKind::Class => vec!["new", "instance", "into_instance"],
            GuestFacadeKind::Module => vec!["new", "module", "into_module"],
        }.into_iter().map(str::to_owned).collect::<Vec<_>>();

        while !contents.is_empty() {
            let member = contents.parse::<GuestMember>()?;

            if names.contains(&member.ident().to_string()) {
                return Err(syn::Error::new(
                    member.ident().span(),
                    "duplicate facade method or generated accessor name",
                ));
            }

            names.push(member.ident().to_string());
            members.push(member);
        }

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after facade"));
        }

        let (krate, payload) = FacadeOptions::resolve(&mut attributes, kind)?;

        Ok(Self {
            attributes,
            visibility,
            name,
            krate,
            payload,
            members,
        })
    }

    pub fn into_parts(self) -> (
        Vec<Attribute>,
        Visibility,
        Ident,
        Path,
        Option<Path>,
        Vec<GuestMember>,
    ) {
        (
            self.attributes,
            self.visibility,
            self.name,
            self.krate,
            self.payload,
            self.members,
        )
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::guest::facade::{
        GuestFacadeKind,
        GuestFacadeSyntax,
    };

    #[test]
    fn rejects_trailing_tokens_and_accessor_collisions() {
        assert!(GuestFacadeSyntax::new(
            quote!(
                #[guestjs(crate_path = crate)]
                pub class Client {} extra
            ),
            GuestFacadeKind::Class,
        ).is_err());
        assert!(GuestFacadeSyntax::new(
            quote!(
                #[guestjs(crate_path = crate)]
                pub class Client { fn instance() -> String; }
            ),
            GuestFacadeKind::Class,
        ).is_err());
    }
}
