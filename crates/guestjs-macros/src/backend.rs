use darling::FromMeta;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    GenericParam, Generics, Ident, LifetimeParam, Meta, Path, Token, Type, TypeParamBound,
    WherePredicate,
    parse::{ParseStream, Parser},
    parse_quote, parse2,
};

pub enum BackendOption {
    Named(Ident),
    Pinned(Type),
}

impl FromMeta for BackendOption {
    fn from_meta(meta: &Meta) -> darling::Result<Self> {
        match meta {
            Meta::NameValue(pair) => {
                parse2::<Ident>(pair.value.to_token_stream())
                    .map(Self::Named)
                    .map_err(|_| {
                        darling::Error::custom(
                            "backend = <name> requires an identifier; use backend(pin = <type>) for a concrete backend",
                        )
                        .with_span(&pair.value)
                    })
            }
            Meta::List(list) => {
                Parser::parse2(
                    |input: ParseStream| {
                        let key = input.parse::<Ident>()?;

                        if key != "pin" {
                            return Err(syn::Error::new(
                                key.span(),
                                "expected pin = <type>",
                            ));
                        }

                        input.parse::<Token![=]>()?;

                        let ty = input.parse::<Type>()?;

                        if !input.is_empty() {
                            return Err(input.error("unexpected backend option"));
                        }

                        Ok(Self::Pinned(ty))
                    },
                    list.tokens.clone(),
                )
                .map_err(|error| darling::Error::from(error).with_span(list))
            }
            Meta::Path(_) => {
                Err(darling::Error::unsupported_format("word").with_span(meta))
            }
        }
    }
}

pub enum BackendParameter {
    Synthesized(Ident),
    Declared(Ident),
    Introduced(Ident),
    Concrete(Type),
}

impl BackendParameter {
    pub fn resolve(
        option: Option<BackendOption>,
        generics: &Generics,
        methods: &[Generics],
    ) -> syn::Result<Self> {
        match option {
            Some(BackendOption::Named(ident)) => {
                if generics
                    .type_params()
                    .any(|parameter| parameter.ident == ident)
                {
                    return Ok(Self::Declared(ident));
                }

                if Self::occupied(generics, &ident) {
                    return Err(syn::Error::new(
                        ident.span(),
                        "backend must name a type parameter",
                    ));
                }

                return Ok(Self::Introduced(ident));
            }
            Some(BackendOption::Pinned(ty)) => return Ok(Self::Concrete(ty)),
            None => {}
        }

        if let Some(parameter) = generics
            .type_params()
            .find(|parameter| {
                parameter
                    .bounds
                    .iter()
                    .any(Self::is_backend)
                    || generics
                        .where_clause
                        .iter()
                        .flat_map(|clause| &clause.predicates)
                        .any(|predicate| match predicate {
                            WherePredicate::Type(predicate) => {
                                matches!(
                                    &predicate.bounded_ty,
                                    Type::Path(path)
                                        if path.qself.is_none()
                                            && path.path.is_ident(
                                                &parameter.ident,
                                            )
                                ) && predicate
                                    .bounds
                                    .iter()
                                    .any(Self::is_backend)
                            }
                            _ => false,
                        })
            })
        {
            return Err(syn::Error::new(
                parameter.ident.span(),
                format!("select the backend explicitly with backend = {}", parameter.ident,),
            ));
        }

        let mut index = 0_u64;

        loop {
            let ident = Ident::new(
                &match index {
                    0 => "B".to_owned(),
                    1 => "__GuestjsBackend".to_owned(),
                    _ => format!("__GuestjsBackend{}", index - 1),
                },
                Span::mixed_site(),
            );

            if !Self::occupied(generics, &ident)
                && methods
                    .iter()
                    .all(|method| !Self::occupied(method, &ident))
            {
                return Ok(Self::Synthesized(ident));
            }

            index += 1;
        }
    }

    fn occupied(generics: &Generics, ident: &Ident) -> bool {
        generics
            .params
            .iter()
            .any(|parameter| match parameter {
                GenericParam::Lifetime(parameter) => parameter.lifetime.ident == *ident,
                GenericParam::Type(parameter) => parameter.ident == *ident,
                GenericParam::Const(parameter) => parameter.ident == *ident,
            })
    }

    fn is_backend(bound: &TypeParamBound) -> bool {
        matches!(
            bound,
            TypeParamBound::Trait(bound)
                if bound
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Backend")
        )
    }

    pub fn ty(&self) -> Type {
        match self {
            Self::Synthesized(ident) | Self::Declared(ident) | Self::Introduced(ident) => {
                parse_quote!(#ident)
            }
            Self::Concrete(ty) => ty.clone(),
        }
    }

    pub fn introduced(&self) -> Option<&Ident> {
        match self {
            Self::Introduced(ident) => Some(ident),
            _ => None,
        }
    }

    pub fn rewrite_method(&self, generics: &mut Generics, krate: &Path) {
        if let Self::Introduced(ident) = self {
            if !generics
                .type_params()
                .any(|parameter| parameter.ident == *ident)
            {
                generics
                    .params
                    .push(parse_quote!(#ident));
            }

            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ident: #krate::backend::Backend));
        }
    }

    pub fn method_generics(&self) -> TokenStream {
        match self {
            Self::Synthesized(ident) | Self::Introduced(ident) => {
                quote!(<#ident>)
            }
            _ => TokenStream::new(),
        }
    }

    pub fn definition_generics(&self, generics: &Generics, bounds: &BackendBounds) -> Generics {
        let mut definition = generics.clone();

        if let Self::Synthesized(ident) | Self::Introduced(ident) = self {
            definition
                .params
                .push(parse_quote!(#ident));
        }

        bounds.apply(&mut definition);

        definition
    }

    pub fn turbofish(&self) -> TokenStream {
        match self {
            Self::Introduced(ident) => quote!(::<#ident>),
            _ => TokenStream::new(),
        }
    }
}

pub struct BackendBounds {
    backend: Type,
    bounds: Vec<TypeParamBound>,
    predicates: Vec<WherePredicate>,
}

impl BackendBounds {
    pub fn new(backend: &BackendParameter, bounds: Vec<TypeParamBound>) -> Self {
        Self {
            backend: backend.ty(),
            bounds,
            predicates: Vec::new(),
        }
    }

    pub fn push(&mut self, bound: TypeParamBound) {
        if !self.bounds.contains(&bound) {
            self.bounds.push(bound);
        }
    }

    pub fn absorb(&mut self, generics: &Generics) {
        for predicate in generics
            .where_clause
            .iter()
            .flat_map(|clause| &clause.predicates)
        {
            let WherePredicate::Type(predicate) = predicate else {
                continue;
            };

            if predicate.bounded_ty != self.backend {
                continue;
            }

            let mut predicate = predicate.clone();

            if generics.lifetimes().next().is_some() {
                let lifetimes = predicate
                    .lifetimes
                    .get_or_insert_with(|| parse_quote!(for<>));

                for parameter in generics.lifetimes() {
                    lifetimes
                        .lifetimes
                        .push(GenericParam::Lifetime(LifetimeParam::new(
                            parameter.lifetime.clone(),
                        )));
                }
            }

            if !self
                .predicates
                .contains(&WherePredicate::Type(predicate.clone()))
            {
                self.predicates
                    .push(WherePredicate::Type(predicate));
            }
        }
    }

    pub fn apply(&self, generics: &mut Generics) {
        if !self.bounds.is_empty() {
            let backend = &self.backend;
            let bounds = &self.bounds;

            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#backend: #(#bounds)+*));
        }

        if !self.predicates.is_empty() {
            generics
                .make_where_clause()
                .predicates
                .extend(self.predicates.iter().cloned());
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{
        AngleBracketedGenericArguments, Generics, ItemImpl, ItemStruct, parse_quote, parse2,
    };

    use crate::backend::{BackendBounds, BackendOption, BackendParameter};

    struct GenericsFixture;

    impl GenericsFixture {
        fn parse(tokens: proc_macro2::TokenStream) -> Generics {
            parse2::<ItemStruct>(quote!(struct Example #tokens;))
                .unwrap()
                .generics
        }
    }

    #[test]
    fn synthesizes_a_backend_without_rewriting_source_generics() {
        let source = parse_quote!(<T>);
        let backend = BackendParameter::resolve(None, &source, &[]).unwrap();

        assert_eq!(backend.ty(), parse_quote!(B));
        assert_eq!(
            backend.definition_generics(
                &source,
                &BackendBounds::new(&backend, vec![parse_quote!(crate::backend::Backend)],),
            ),
            GenericsFixture::parse(quote!(
                <T, B> where B: crate::backend::Backend
            )),
        );
        assert_eq!(source, parse_quote!(<T>));
    }

    #[test]
    fn avoids_all_occupied_generated_names() {
        let backend = BackendParameter::resolve(
            None,
            &parse_quote!(<B, __GuestjsBackend>),
            &[parse_quote!(<__GuestjsBackend1>)],
        )
        .unwrap();

        assert_eq!(backend.ty(), parse_quote!(__GuestjsBackend2));
    }

    #[test]
    fn reuses_an_explicit_declared_backend() {
        let backend = BackendParameter::resolve(
            Some(BackendOption::Named(parse_quote!(Engine))),
            &parse_quote!(<Engine>),
            &[],
        )
        .unwrap();

        assert_eq!(backend.ty(), parse_quote!(Engine));
        assert!(backend.method_generics().is_empty());
        assert!(backend.turbofish().is_empty());
    }

    #[test]
    fn introduces_an_explicit_method_backend() {
        let backend = BackendParameter::resolve(
            Some(BackendOption::Named(parse_quote!(Engine))),
            &Generics::default(),
            &[],
        )
        .unwrap();
        let mut method = parse_quote!(<'a>);

        backend.rewrite_method(&mut method, &parse_quote!(crate));

        assert_eq!(
            method,
            GenericsFixture::parse(quote!(
                <'a, Engine> where Engine: crate::backend::Backend
            )),
        );
        assert_eq!(
            parse2::<AngleBracketedGenericArguments>(backend.turbofish(),).unwrap(),
            parse_quote!(::<Engine>),
        );
    }

    #[test]
    fn concrete_backends_add_no_generic_parameter() {
        let backend = BackendParameter::resolve(
            Some(BackendOption::Pinned(parse_quote!(engine::QuickJs))),
            &parse_quote!(<T>),
            &[],
        )
        .unwrap();

        assert_eq!(backend.ty(), parse_quote!(engine::QuickJs));
        assert!(backend.method_generics().is_empty());
        assert!(backend.turbofish().is_empty());
    }

    #[test]
    fn backend_bounds_require_explicit_selection() {
        for item in [
            parse2::<ItemImpl>(quote!(
                impl<B: crate::backend::Backend> Example<B> {}
            ))
            .unwrap(),
            parse2::<ItemImpl>(quote!(
                impl<B> Example<B> where B: crate::backend::Backend {}
            ))
            .unwrap(),
        ] {
            assert!(BackendParameter::resolve(None, &item.generics, &[],).is_err());
        }
    }

    #[test]
    fn lifted_backend_predicates_bind_method_lifetimes() {
        let backend = BackendParameter::resolve(
            Some(BackendOption::Named(parse_quote!(B))),
            &Generics::default(),
            &[],
        )
        .unwrap();
        let source = GenericsFixture::parse(quote!(
            <'a> where B: Capability<'a>, Other: Clone
        ));
        let mut bounds = BackendBounds::new(&backend, Vec::new());
        let mut generated = Generics::default();

        bounds.absorb(&source);
        bounds.apply(&mut generated);

        assert_eq!(generated, GenericsFixture::parse(quote!(where for<'a> B: Capability<'a>)),);
        assert_eq!(
            source,
            GenericsFixture::parse(quote!(
                <'a> where B: Capability<'a>, Other: Clone
            )),
        );
    }
}
