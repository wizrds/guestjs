pub mod callable;
pub mod class;
pub mod exception;
pub mod module;
pub mod parameter;
pub mod receiver;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Generics, Ident, ImplItem, ItemImpl, Meta, Path, Token, Type, parse::Parser, parse_quote,
    punctuated::Punctuated,
};

use crate::{
    backend::{BackendBounds, BackendParameter},
    naming::{Naming, RenameRule},
};

#[derive(Default)]
pub struct TypeList {
    types: Vec<Type>,
}

impl TypeList {
    pub fn types(&self) -> &[Type] {
        &self.types
    }
}

impl FromMeta for TypeList {
    fn from_meta(meta: &Meta) -> darling::Result<Self> {
        let Meta::List(list) = meta else {
            return Err(darling::Error::unsupported_format("list").with_span(meta));
        };

        Ok(Self {
            types: Punctuated::<Type, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())?
                .into_iter()
                .collect(),
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MemberKey {
    Name(String),
    Symbol(Ident),
}

impl MemberKey {
    pub fn new(
        ident: &Ident,
        name: Option<String>,
        symbol: Option<&str>,
        rename: Option<RenameRule>,
    ) -> syn::Result<Self> {
        if name.is_some() && symbol.is_some() {
            return Err(syn::Error::new(ident.span(), "name and symbol cannot be combined"));
        }

        match symbol {
            None => Ok(Self::Name(Naming::member(ident, name, rename))),
            Some(symbol) => Ok(Self::Symbol(syn::Ident::new(
                match symbol {
                    "iterator" => "Iterator",
                    "asyncIterator" => "AsyncIterator",
                    "toPrimitive" => "ToPrimitive",
                    "toStringTag" => "ToStringTag",
                    "hasInstance" => "HasInstance",
                    _ => {
                        return Err(syn::Error::new(
                            ident.span(),
                            "unknown JavaScript well-known symbol",
                        ));
                    }
                },
                ident.span(),
            ))),
        }
    }

    pub fn render(&self, krate: &Path) -> TokenStream {
        match self {
            Self::Name(name) => quote!(#name),
            Self::Symbol(symbol) => {
                quote!(#krate::backend::values::WellKnown::#symbol)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Data,
    Get,
    Set,
}

#[derive(Default)]
pub struct MemberRegistry {
    keys: Vec<(MemberKey, Registration)>,
}

impl MemberRegistry {
    pub fn insert(&mut self, key: MemberKey, role: Registration, ident: &Ident) -> syn::Result<()> {
        if self
            .keys
            .iter()
            .any(|(existing, previous)| {
                *existing == key
                    && (*previous == role
                        || *previous == Registration::Data
                        || role == Registration::Data)
            })
        {
            return Err(syn::Error::new(ident.span(), "duplicate guest member registration"));
        }

        self.keys.push((key, role));

        Ok(())
    }
}

#[derive(Default)]
pub struct StateRequirements {
    types: Vec<Type>,
}

impl StateRequirements {
    pub fn extend(&mut self, types: impl IntoIterator<Item = Type>) {
        for ty in types {
            if !self.types.contains(&ty) {
                self.types.push(ty);
            }
        }
    }

    pub fn render(&self) -> Vec<TokenStream> {
        self.types
            .iter()
            .map(|ty| quote!(.require::<#ty>()))
            .collect()
    }
}

pub struct HostSyntax;

impl HostSyntax {
    pub fn target(item: &ItemImpl) -> syn::Result<Ident> {
        if item.trait_.is_some() {
            return Err(syn::Error::new_spanned(item, "host attributes require an inherent impl"));
        }

        let Type::Path(target) = item.self_ty.as_ref() else {
            return Err(syn::Error::new_spanned(
                item.self_ty.as_ref(),
                "host attributes require a named type",
            ));
        };

        if target.qself.is_some() {
            return Err(syn::Error::new_spanned(target, "host attributes require a named type"));
        }

        target
            .path
            .segments
            .last()
            .map(|segment| segment.ident.clone())
            .ok_or_else(|| syn::Error::new_spanned(target, "missing type name"))
    }

    pub fn exported_generics(item: &ItemImpl) -> Vec<Generics> {
        item.items
            .iter()
            .filter_map(|member| match member {
                ImplItem::Fn(method)
                    if method
                        .attrs
                        .iter()
                        .any(|attribute| attribute.path().is_ident("guestjs")) =>
                {
                    Some(method.sig.generics.clone())
                }
                _ => None,
            })
            .collect()
    }

    pub fn rewrite(
        item: &mut ItemImpl,
        names: &[Ident],
        backend: &BackendParameter,
        bounds: &mut BackendBounds,
        krate: &Path,
    ) {
        for member in &mut item.items {
            if let ImplItem::Fn(method) = member {
                if names.contains(&method.sig.ident) {
                    if backend.introduced().is_some() {
                        bounds.absorb(&method.sig.generics);
                    }

                    backend.rewrite_method(&mut method.sig.generics, krate);
                }
            }
        }
    }

    pub fn payload_generics(item: &ItemImpl) -> Generics {
        let mut generics = item.generics.clone();
        let target = item.self_ty.as_ref();

        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#target: 'static));

        generics
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use crate::host::{HostSyntax, MemberKey, MemberRegistry, Registration, StateRequirements};

    #[test]
    fn only_named_inherent_impls_are_accepted() {
        assert!(HostSyntax::target(&parse_quote!(impl Counter {})).is_ok());
        assert!(HostSyntax::target(&parse_quote!(impl Trait for Counter {})).is_err());
        assert!(
            HostSyntax::target(&parse_quote!(
                impl (Counter,) {}
            ))
            .is_err()
        );
    }

    #[test]
    fn accessor_pairs_share_a_key_without_allowing_duplicates() {
        let mut registry = MemberRegistry::default();
        let key = MemberKey::new(&parse_quote!(value), None, None, None).unwrap();

        assert!(
            registry
                .insert(key.clone(), Registration::Get, &parse_quote!(value),)
                .is_ok()
        );
        assert!(
            registry
                .insert(key.clone(), Registration::Set, &parse_quote!(set_value),)
                .is_ok()
        );
        assert!(
            registry
                .insert(key, Registration::Get, &parse_quote!(other),)
                .is_err()
        );
    }

    #[test]
    fn requirement_types_are_deduplicated_structurally() {
        let mut requirements = StateRequirements::default();

        requirements.extend([
            parse_quote!(ModuleState<First>),
            parse_quote!(ModuleState<First>),
            parse_quote!(ModuleState<Second>),
        ]);

        assert_eq!(requirements.render().len(), 2);
    }
}
