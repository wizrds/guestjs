use std::collections::HashMap;

use darling::{FromMeta, ast::NestedMeta, util::Flag};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Generics, Ident, ImplItem, ImplItemConst, ItemImpl, Path, Type, spanned::Spanned};

use crate::{
    host::{
        HostMacroError,
        attributes::HelperAttributes,
        callable::{Callable, CallableKind, ClassMethod, StaticsHook, WellKnownSymbol},
        naming::{Naming, RenameRule},
    },
    path::CratePath,
};

#[derive(FromMeta)]
struct ClassOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ConstantOptions {
    constant: Flag,
    name: Option<String>,
}

struct StaticConstant {
    span: Span,
    ident: Ident,
    name: String,
    value_type: Type,
}

impl StaticConstant {
    fn new(
        constant: &mut ImplItemConst,
        rename_all: Option<RenameRule>,
    ) -> Result<Option<Self>, HostMacroError> {
        let helpers = HelperAttributes::take(&mut constant.attrs)?;

        if helpers.is_empty() {
            return Ok(None);
        }

        let options = ConstantOptions::from_list(&helpers)?;

        if !options.constant.is_present() {
            return Err(syn::Error::new(
                constant.ident.span(),
                "an exported associated constant requires the constant role",
            )
            .into());
        }

        Ok(Some(Self {
            span: constant.ident.span(),
            ident: constant.ident.clone(),
            name: Naming::member(&constant.ident, options.name, rename_all),
            value_type: constant.ty.clone(),
        }))
    }

    fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        target: &Type,
    ) {
        let value_type = &self.value_type;

        if value_type == target {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(#value_type: Clone + 'static));
        } else {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(
                    #value_type: #crate_path::marshal::ToGuest + Clone + 'static
                ));
        }
    }

    fn registration(&self) -> TokenStream {
        let ident = &self.ident;
        let name = &self.name;

        quote! {
            statics.constant(#name, Self::#ident);
        }
    }
}

struct Accessor {
    name: String,
    getter: Option<Callable>,
    setter: Option<Callable>,
}

impl Accessor {
    fn new(callable: Callable) -> Self {
        let name = callable.name().to_owned();

        match callable.kind() {
            CallableKind::Getter => Self {
                name,
                getter: Some(callable),
                setter: None,
            },
            CallableKind::Setter => Self {
                name,
                getter: None,
                setter: Some(callable),
            },
            _ => unreachable!(),
        }
    }

    fn insert(&mut self, callable: Callable) -> Result<(), HostMacroError> {
        let (slot, kind) = match callable.kind() {
            CallableKind::Getter => (&mut self.getter, "getter"),
            CallableKind::Setter => (&mut self.setter, "setter"),
            _ => unreachable!(),
        };

        if let Some(previous) = slot {
            let mut error = syn::Error::new(
                callable.span(),
                format!("duplicate {kind} for accessor {:?}", self.name),
            );

            error.combine(syn::Error::new(
                previous.span(),
                format!("the first {kind} for this accessor is here"),
            ));

            return Err(error.into());
        }

        *slot = Some(callable);

        Ok(())
    }

    fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        target: &Type,
    ) {
        if let Some(getter) = &self.getter {
            getter.add_predicates(generics, crate_path, target);
        }

        if let Some(setter) = &self.setter {
            setter.add_predicates(generics, crate_path, target);
        }
    }

    fn registration(&self, crate_path: &Path) -> TokenStream {
        let name = &self.name;

        match (&self.getter, &self.setter) {
            (Some(getter), Some(setter)) => {
                let getter = getter.getter_closure();
                let setter_closure = setter.setter_closure();
                let setter_descriptor = setter.setter_descriptor(crate_path);

                quote! {
                    spec.accessor::<_, _, _, #setter_descriptor>(
                        #name,
                        #getter,
                        #setter_closure,
                    );
                }
            }
            (Some(getter), None) => {
                let getter = getter.getter_closure();

                quote! {
                    spec.getter(#name, #getter);
                }
            }
            (None, Some(setter)) => {
                let setter_closure = setter.setter_closure();
                let setter_descriptor = setter.setter_descriptor(crate_path);

                quote! {
                    spec.setter::<_, #setter_descriptor>(
                        #name,
                        #setter_closure,
                    );
                }
            }
            (None, None) => unreachable!(),
        }
    }
}

enum StaticMember {
    Constant(Box<StaticConstant>),
    Method(Box<Callable>),
    Hook(StaticsHook),
}

impl StaticMember {
    fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        target: &Type,
    ) {
        match self {
            Self::Constant(constant) => {
                constant.add_predicates(generics, crate_path, target);
            }
            Self::Method(method) => {
                method.add_predicates(generics, crate_path, target);
            }
            Self::Hook(_) => {}
        }
    }

    fn registration(&self, crate_path: &Path) -> TokenStream {
        match self {
            Self::Constant(constant) => constant.registration(),
            Self::Method(method) => method.registration(crate_path),
            Self::Hook(hook) => hook.registration(),
        }
    }
}

pub(crate) struct HostClassMacro {
    item: ItemImpl,
    name: String,
    crate_path: Path,
    constructor: Callable,
    methods: Vec<Callable>,
    accessors: Vec<Accessor>,
    statics: Vec<StaticMember>,
}

impl HostClassMacro {
    pub(crate) fn new(
        args: TokenStream,
        mut item: ItemImpl,
    ) -> Result<Self, HostMacroError> {
        Self::validate_impl(&item)?;

        let options = ClassOptions::from_list(
            &NestedMeta::parse_meta_list(args)?,
        )?;
        let mut constructor = None;
        let mut methods = Vec::new();
        let mut accessors = Vec::<Accessor>::new();
        let mut statics = Vec::new();
        let mut accessor_indices = HashMap::<String, usize>::new();
        let mut prototype_names = HashMap::new();
        let mut static_names = HashMap::new();
        let mut symbols = HashMap::new();
        let mut statics_hook = None;

        for item in &mut item.items {
            match item {
                ImplItem::Fn(method) => {
                    let Some(member) = ClassMethod::new(method, options.rename_all)? else {
                        continue;
                    };

                    match member {
                        ClassMethod::Callable(callable) => match callable.kind() {
                            CallableKind::Constructor => {
                                if constructor.is_some() {
                                    return Err(syn::Error::new(
                                        callable.span(),
                                        "a host class may have only one constructor",
                                    )
                                    .into());
                                }

                                constructor = Some(*callable);
                            }
                            CallableKind::Getter | CallableKind::Setter => {
                                if let Some(index) = accessor_indices.get(callable.name()) {
                                    accessors[*index].insert(*callable)?;
                                } else {
                                    Self::insert_name(
                                        &mut prototype_names,
                                        callable.name(),
                                        callable.span(),
                                        "prototype member",
                                    )?;

                                    accessor_indices.insert(
                                        callable.name().to_owned(),
                                        accessors.len(),
                                    );
                                    accessors.push(Accessor::new(*callable));
                                }
                            }
                            CallableKind::AsyncMethod | CallableKind::Method => {
                                Self::insert_name(
                                    &mut prototype_names,
                                    callable.name(),
                                    callable.span(),
                                    "prototype member",
                                )?;

                                methods.push(*callable);
                            }
                            CallableKind::Iterable => {
                                Self::insert_symbol(
                                    &mut symbols,
                                    WellKnownSymbol::Iterator,
                                    callable.span(),
                                )?;

                                methods.push(*callable);
                            }
                            CallableKind::Symbol(symbol) => {
                                Self::insert_symbol(
                                    &mut symbols,
                                    symbol,
                                    callable.span(),
                                )?;

                                methods.push(*callable);
                            }
                            CallableKind::StaticMethod => {
                                Self::insert_name(
                                    &mut static_names,
                                    callable.name(),
                                    callable.span(),
                                    "static member",
                                )?;

                                statics.push(StaticMember::Method(callable));
                            }
                        },
                        ClassMethod::Statics(hook) => {
                            if let Some(previous) = statics_hook {
                                let mut error = syn::Error::new(
                                    hook.span(),
                                    "a host class may have only one statics hook",
                                );

                                error.combine(syn::Error::new(
                                    previous,
                                    "the first statics hook is here",
                                ));

                                return Err(error.into());
                            }

                            statics_hook = Some(hook.span());
                            statics.push(StaticMember::Hook(hook));
                        }
                    }
                }
                ImplItem::Const(constant) => {
                    let Some(constant) = StaticConstant::new(
                        constant,
                        options.rename_all,
                    )? else {
                        continue;
                    };

                    Self::insert_name(
                        &mut static_names,
                        &constant.name,
                        constant.span,
                        "static member",
                    )?;

                    statics.push(StaticMember::Constant(Box::new(constant)));
                }
                _ => {}
            }
        }

        Ok(Self {
            constructor: constructor.ok_or_else(|| {
                syn::Error::new(
                    item.impl_token.span(),
                    "a host class requires exactly one constructor",
                )
            })?,
            name: match options.name {
                Some(name) => name,
                None => Self::default_name(&item)?,
            },
            item,
            crate_path: CratePath::new(options.crate_path).resolve()?,
            methods,
            accessors,
            statics,
        })
    }

    fn validate_impl(item: &ItemImpl) -> Result<(), HostMacroError> {
        if item.trait_.is_some() {
            return Err(syn::Error::new(
                item.impl_token.span(),
                "host_class applies only to inherent implementations",
            )
            .into());
        }

        if item.unsafety.is_some() {
            return Err(syn::Error::new(
                item.unsafety.span(),
                "unsafe host class implementations are not supported",
            )
            .into());
        }

        if item.defaultness.is_some() {
            return Err(syn::Error::new(
                item.defaultness.span(),
                "specialized host class implementations are not supported",
            )
            .into());
        }

        Self::target_ident(item).map(|_| ())
    }

    fn default_name(item: &ItemImpl) -> Result<String, HostMacroError> {
        Ok(Self::target_ident(item)?.to_string())
    }

    fn target_ident(item: &ItemImpl) -> Result<&Ident, HostMacroError> {
        let Type::Path(target) = item.self_ty.as_ref() else {
            return Err(syn::Error::new_spanned(
                item.self_ty.as_ref(),
                "host_class requires a nominal type target",
            )
            .into());
        };
        if target.qself.is_some() {
            return Err(syn::Error::new_spanned(
                target,
                "host_class requires a nominal type target",
            )
            .into());
        }
        let Some(segment) = target.path.segments.last() else {
            return Err(syn::Error::new_spanned(
                target,
                "host_class requires a named type target",
            )
            .into());
        };

        Ok(&segment.ident)
    }

    fn insert_name(
        names: &mut HashMap<String, Span>,
        name: &str,
        span: Span,
        kind: &str,
    ) -> Result<(), HostMacroError> {
        let Some(previous) = names.insert(name.to_owned(), span) else {
            return Ok(());
        };
        let mut error = syn::Error::new(
            span,
            format!("duplicate guest-visible {kind} name {name:?}"),
        );

        error.combine(syn::Error::new(
            previous,
            format!("the first {kind} with this guest-visible name is here"),
        ));

        Err(error.into())
    }

    fn insert_symbol(
        symbols: &mut HashMap<WellKnownSymbol, Span>,
        symbol: WellKnownSymbol,
        span: Span,
    ) -> Result<(), HostMacroError> {
        let Some(previous) = symbols.insert(symbol, span) else {
            return Ok(());
        };
        let mut error = syn::Error::new(
            span,
            "duplicate guest-visible well-known symbol",
        );

        error.combine(syn::Error::new(
            previous,
            "the first method for this well-known symbol is here",
        ));

        Err(error.into())
    }

    pub(crate) fn expand(self) -> TokenStream {
        let Self {
            item,
            name,
            crate_path,
            constructor,
            methods,
            accessors,
            statics,
        } = self;
        let target = item.self_ty.as_ref();
        let mut generics = item.generics.clone();

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#target: 'static));
        constructor.add_predicates(&mut generics, &crate_path, target);

        for method in &methods {
            method.add_predicates(&mut generics, &crate_path, target);
        }

        for accessor in &accessors {
            accessor.add_predicates(&mut generics, &crate_path, target);
        }

        for member in &statics {
            member.add_predicates(&mut generics, &crate_path, target);
        }

        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let scope = if constructor.uses_scope() {
            quote!(scope)
        } else {
            quote!(_scope)
        };
        let args = if constructor.uses_args() {
            quote!(args)
        } else {
            quote!(_args)
        };
        let construct = constructor.construct(&crate_path);
        let registrations = methods
            .iter()
            .map(|method| method.registration(&crate_path));
        let accessor_registrations = accessors
            .iter()
            .map(|accessor| accessor.registration(&crate_path));
        let static_registrations = statics
            .iter()
            .map(|member| member.registration(&crate_path));
        let static_registration = if statics.is_empty() {
            quote!()
        } else {
            quote! {
                spec.statics(|statics| {
                    #(#static_registrations)*
                });
            }
        };
        let spec = if methods.is_empty() && accessors.is_empty() && statics.is_empty() {
            quote!(_spec)
        } else {
            quote!(spec)
        };

        quote! {
            #item

            impl #impl_generics #crate_path::host::HostClass for #target
                #where_clause
            {
                const NAME: &'static str = #name;

                fn construct<'js>(
                    #scope: &#crate_path::runtime::Scope<'js>,
                    #args: #crate_path::host::Args<'js>,
                ) -> Result<Self, #crate_path::errors::Error> {
                    #construct
                }

                fn build(
                    #spec: &mut #crate_path::host::ClassSpec<Self>,
                ) {
                    #(#registrations)*
                    #(#accessor_registrations)*
                    #static_registration
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use crate::host::class::HostClassMacro;

    #[test]
    fn generates_constructor_and_receiver_specific_registrations() {
        let output = HostClassMacro::new(
            quote!(rename_all = "camelCase", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestjs(constructor)]
                    fn new(value: i32) -> Result<Self, DomainError> {
                        Ok(Self(value))
                    }

                    #[guestjs(method)]
                    fn read_value(&self) -> Result<i32, DomainError> {
                        Ok(self.0)
                    }

                    #[guestjs(method, name = "increment")]
                    fn add(&mut self, value: i32) -> Result<i32, DomainError> {
                        self.0 += value;

                        Ok(self.0)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("const NAME : & 'static str = \"Counter\""));
        assert!(output.contains("spec . method (\"readValue\""));
        assert!(output.contains("spec . method_mut (\"increment\""));
        assert!(output.contains("map_err (Into :: into)"));
    }

    #[test]
    fn generates_accessors_symbols_and_iteration() {
        let accessors = HostClassMacro::new(
            quote!(name = "Counter", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestjs(get)]
                    fn value(&self) -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(set, name = "value")]
                    fn set_value(&mut self, value: i32) -> Result<(), Error> {
                        Ok(())
                    }

                    #[guestjs(get)]
                    fn readable(&self) -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(set)]
                    fn writable(&mut self, value: i32) -> Result<(), Error> {
                        Ok(())
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(accessors.contains("spec . accessor :: < _ , _ , _ , i32 >"));
        assert!(accessors.contains("spec . getter (\"readable\""));
        assert!(accessors.contains("spec . setter :: < _ , i32 >"));

        let symbols = HostClassMacro::new(
            quote!(name = "Symbols", crate_path = crate),
            parse_quote! {
                impl Symbols {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestjs(symbol = "iterator")]
                    fn iterator(&self) -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(symbol = "asyncIterator")]
                    fn async_iterator(&self) -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(symbol = "toPrimitive")]
                    fn to_primitive(&self) -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(symbol = "hasInstance")]
                    fn has_instance(&self) -> Result<i32, Error> {
                        Ok(1)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(symbols.contains("WellKnownSymbol :: Iterator"));
        assert!(symbols.contains("WellKnownSymbol :: AsyncIterator"));
        assert!(symbols.contains("WellKnownSymbol :: ToPrimitive"));
        assert!(symbols.contains("WellKnownSymbol :: HasInstance"));

        let iterable = HostClassMacro::new(
            quote!(name = "Iterable", crate_path = crate),
            parse_quote! {
                impl Iterable {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestjs(iterable)]
                    fn values(&self) -> Result<Vec<i32>, Error> {
                        Ok(vec![1, 2])
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(iterable.contains("spec . iterable"));
        assert!(iterable.contains("Vec < i32 > : IntoIterator"));
    }

    #[test]
    fn preserves_static_member_order() {
        let output = HostClassMacro::new(
            quote!(name = "Counter", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestjs(constant)]
                    const BEFORE: i32 = 1;

                    #[guestjs(static_method)]
                    fn from_method() -> Result<i32, Error> {
                        Ok(2)
                    }

                    #[guestjs(statics)]
                    fn add_statics(statics: &mut Namespace) {
                        statics.constant("hook", 3);
                    }

                    #[guestjs(constant)]
                    const AFTER: i32 = 4;
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("spec . statics"));
        assert!(
            output.find("BEFORE").unwrap()
                < output.find("from_method").unwrap(),
        );
        assert!(
            output.find("from_method").unwrap()
                < output.find("add_statics").unwrap(),
        );
        assert!(
            output.find("add_statics").unwrap()
                < output.find("AFTER").unwrap(),
        );
    }

    #[test]
    fn generates_shared_and_exclusive_owned_futures() {
        let output = HostClassMacro::new(
            quote!(name = "Counter", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestjs(async_method)]
                    fn read(
                        &self,
                    ) -> Result<
                        impl Future<Output = Result<i32, InnerError>> + 'static,
                        OuterError,
                    > {
                        Ok(async move { Ok(1) })
                    }

                    #[guestjs(async_method)]
                    fn write(
                        &mut self,
                    ) -> Result<
                        impl Future<Output = Result<i32, InnerError>> + 'static,
                        OuterError,
                    > {
                        Ok(async move { Ok(1) })
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("spec . async_method (\"read\""));
        assert!(output.contains("spec . async_method_mut (\"write\""));
        assert!(output.contains("future . await . map_err (Into :: into)"));
        assert!(output.contains("std :: pin :: Pin"));
        assert!(output.contains("dyn :: std :: future :: Future"));
        assert!(output.contains("InnerError : Into < crate :: errors :: Error >"));
    }

    #[test]
    fn preserves_unannotated_methods_and_unrelated_attributes() {
        let output = HostClassMacro::new(
            quote!(name = "Counter", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestjs(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[allow(dead_code)]
                    fn helper(&self) {}
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("allow (dead_code)"));
        assert!(output.contains("fn helper"));
    }

    #[test]
    fn preserves_outer_generics_and_bounds() {
        let output = HostClassMacro::new(
            quote!(name = "Wrapper", crate_path = crate),
            parse_quote! {
                impl<T> Wrapper<T>
                where
                    T: Clone,
                {
                    #[guestjs(constructor)]
                    fn new(value: T) -> Result<Self, Error> {
                        Ok(Self(value))
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("impl < T > crate :: host :: HostClass"));
        assert!(output.contains("T : Clone"));
        assert!(output.contains("T : crate :: marshal :: FromGuestBound"));
        assert!(output.contains("Wrapper < T > : 'static"));
    }

    #[test]
    fn rejects_invalid_class_definitions() {
        assert!(
            HostClassMacro::new(
                quote!(name = "Missing", crate_path = crate),
                parse_quote!(impl Missing {}),
            )
            .is_err(),
        );
        assert!(
            HostClassMacro::new(
                quote!(name = "Duplicate", crate_path = crate),
                parse_quote! {
                    impl Duplicate {
                        #[guestjs(constructor)]
                        fn first() -> Result<Self, Error> {
                            Ok(Self)
                        }

                        #[guestjs(constructor)]
                        fn second() -> Result<Self, Error> {
                            Ok(Self)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostClassMacro::new(
                quote!(name = "Duplicate", crate_path = crate),
                parse_quote! {
                    impl Duplicate {
                        #[guestjs(constructor)]
                        fn new() -> Result<Self, Error> {
                            Ok(Self)
                        }

                        #[guestjs(method, name = "same")]
                        fn first(&self) -> Result<(), Error> {
                            Ok(())
                        }

                        #[guestjs(method, name = "same")]
                        fn second(&self) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostClassMacro::new(
                quote!(name = "Duplicate", crate_path = crate),
                parse_quote! {
                    impl Duplicate {
                        #[guestjs(constructor)]
                        fn new() -> Result<Self, Error> {
                            Ok(Self)
                        }

                        #[guestjs(get, name = "value")]
                        fn first(&self) -> Result<i32, Error> {
                            Ok(1)
                        }

                        #[guestjs(get, name = "value")]
                        fn second(&self) -> Result<i32, Error> {
                            Ok(2)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostClassMacro::new(
                quote!(name = "Duplicate", crate_path = crate),
                parse_quote! {
                    impl Duplicate {
                        #[guestjs(constructor)]
                        fn new() -> Result<Self, Error> {
                            Ok(Self)
                        }

                        #[guestjs(statics)]
                        fn first(statics: &mut Namespace) {}

                        #[guestjs(statics)]
                        fn second(statics: &mut Namespace) {}
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostClassMacro::new(
                quote!(name = "Async", crate_path = crate),
                parse_quote! {
                    impl Async {
                        #[guestjs(constructor)]
                        fn new() -> Result<Self, Error> {
                            Ok(Self)
                        }

                        #[guestjs(async_method)]
                        async fn value(&self) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
    }
}
