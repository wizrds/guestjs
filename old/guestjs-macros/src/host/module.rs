use std::collections::HashMap;

use darling::{
    FromMeta,
    ast::NestedMeta,
    util::{Flag, PathList},
};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Generics, Ident, ImplItem, ImplItemConst, ItemImpl, Path, Type, spanned::Spanned,
};

use crate::{
    host::{
        HostMacroError,
        attributes::HelperAttributes,
        callable::{ModuleFunction, ModuleHook, ModuleHookKind, ModuleMethod},
        naming::{Naming, RenameRule},
    },
    path::CratePath,
};

#[derive(FromMeta)]
struct ModuleOptions {
    name: String,
    #[darling(default)]
    classes: PathList,
    rename_all: Option<RenameRule>,
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ConstantOptions {
    constant: Flag,
    default: Flag,
    name: Option<String>,
}

impl ConstantOptions {
    fn role_count(&self) -> usize {
        [self.constant.is_present(), self.default.is_present()]
            .into_iter()
            .filter(|present| *present)
            .count()
    }
}

#[derive(Clone, Copy)]
enum ModuleConstantKind {
    Constant,
    Default,
}

struct ModuleConstant {
    kind: ModuleConstantKind,
    span: Span,
    ident: Ident,
    name: String,
    value_type: Type,
}

impl ModuleConstant {
    fn new(
        constant: &mut ImplItemConst,
        rename_all: Option<RenameRule>,
    ) -> Result<Option<Self>, HostMacroError> {
        let helpers = HelperAttributes::take(&mut constant.attrs)?;

        if helpers.is_empty() {
            return Ok(None);
        }

        let options = ConstantOptions::from_list(&helpers)?;

        if options.role_count() != 1 {
            return Err(syn::Error::new(
                constant.ident.span(),
                "an exported host module constant requires exactly one constant or default role",
            )
            .into());
        }

        if options.default.is_present() && options.name.is_some() {
            return Err(syn::Error::new(
                constant.ident.span(),
                "a default export cannot have a guest-visible name",
            )
            .into());
        }

        let kind = if options.default.is_present() {
            ModuleConstantKind::Default
        } else {
            ModuleConstantKind::Constant
        };
        let name = match kind {
            ModuleConstantKind::Constant => {
                Naming::member(&constant.ident, options.name, rename_all)
            }
            ModuleConstantKind::Default => String::from("default"),
        };

        Ok(Some(Self {
            kind,
            span: constant.ident.span(),
            ident: constant.ident.clone(),
            name,
            value_type: constant.ty.clone(),
        }))
    }

    fn add_predicates(&self, generics: &mut Generics, crate_path: &Path) {
        let value_type = &self.value_type;

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #value_type: #crate_path::marshal::ToGuest + Clone + 'static
            ));
    }

    fn registration(&self) -> TokenStream {
        let ident = &self.ident;

        match self.kind {
            ModuleConstantKind::Constant => {
                let name = &self.name;

                quote! {
                    exports.constant(#name, Self::#ident);
                }
            }
            ModuleConstantKind::Default => quote! {
                exports.default(Self::#ident);
            },
        }
    }
}

enum ModuleMember {
    Constant(Box<ModuleConstant>),
    Function(Box<ModuleFunction>),
    Hook(Box<ModuleHook>),
}

impl ModuleMember {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Constant(constant) => Some(constant.name.as_str()),
            Self::Function(function) => Some(function.name()),
            Self::Hook(hook) => hook.name(),
        }
    }

    fn span(&self) -> Span {
        match self {
            Self::Constant(constant) => constant.span,
            Self::Function(function) => function.span(),
            Self::Hook(hook) => hook.span(),
        }
    }

    fn add_predicates(&self, generics: &mut Generics, crate_path: &Path) {
        match self {
            Self::Constant(constant) => constant.add_predicates(generics, crate_path),
            Self::Function(function) => function.add_predicates(generics, crate_path),
            Self::Hook(_) => {}
        }
    }

    fn registration(&self, crate_path: &Path) -> TokenStream {
        match self {
            Self::Constant(constant) => constant.registration(),
            Self::Function(function) => function.registration(crate_path),
            Self::Hook(hook) => hook.registration(),
        }
    }
}

pub(crate) struct HostModuleMacro {
    item: ItemImpl,
    name: String,
    crate_path: Path,
    classes: PathList,
    init_hook: Option<Box<ModuleHook>>,
    members: Vec<ModuleMember>,
}

impl HostModuleMacro {
    pub(crate) fn new(args: TokenStream, mut item: ItemImpl) -> Result<Self, HostMacroError> {
        Self::validate_impl(&item)?;

        let options = ModuleOptions::from_list(&NestedMeta::parse_meta_list(args)?)?;
        let mut class_paths = HashMap::new();

        for class in options.classes.iter() {
            Self::insert_name(
                &mut class_paths,
                class.to_token_stream().to_string(),
                class.span(),
                "class path",
            )?;
        }

        let mut export_names = HashMap::new();
        let mut build_hook = None;
        let mut init_hook = None::<Box<ModuleHook>>;
        let mut members = Vec::new();

        for member in &mut item.items {
            let member = match member {
                ImplItem::Fn(method) => {
                    let Some(method) = ModuleMethod::new(method, options.rename_all)? else {
                        continue;
                    };

                    match method {
                        ModuleMethod::Function(function) => ModuleMember::Function(function),
                        ModuleMethod::Hook(hook) => {
                            match (
                                hook.kind(),
                                init_hook
                                    .as_ref()
                                    .map(|hook| hook.span()),
                            ) {
                                (ModuleHookKind::Build, _) => {
                                    if let Some(previous) = build_hook {
                                        let mut error = syn::Error::new(
                                            hook.span(),
                                            "a host module may have only one build hook",
                                        );

                                        error.combine(syn::Error::new(
                                            previous,
                                            "the first build hook is here",
                                        ));

                                        return Err(error.into());
                                    }

                                    build_hook = Some(hook.span());
                                }
                                (ModuleHookKind::Init, Some(previous)) => {
                                    let mut error = syn::Error::new(
                                        hook.span(),
                                        "a host module may have only one init hook",
                                    );

                                    error.combine(syn::Error::new(
                                        previous,
                                        "the first init hook is here",
                                    ));

                                    return Err(error.into());
                                }
                                (ModuleHookKind::Init, None) => {
                                    init_hook = Some(hook);

                                    continue;
                                }
                                (ModuleHookKind::Object, _) => {}
                            }

                            ModuleMember::Hook(hook)
                        }
                    }
                }
                ImplItem::Const(constant) => {
                    let Some(constant) = ModuleConstant::new(constant, options.rename_all)? else {
                        continue;
                    };

                    ModuleMember::Constant(Box::new(constant))
                }
                ImplItem::Type(value_type) => {
                    Self::reject_unsupported_member(
                        &mut value_type.attrs,
                        value_type.ident.span(),
                    )?;

                    continue;
                }
                ImplItem::Macro(item_macro) => {
                    Self::reject_unsupported_member(
                        &mut item_macro.attrs,
                        item_macro.mac.path.span(),
                    )?;

                    continue;
                }
                _ => continue,
            };

            if let Some(name) = member.name() {
                Self::insert_name(
                    &mut export_names,
                    name.to_owned(),
                    member.span(),
                    "export name",
                )?;
            }

            members.push(member);
        }

        Ok(Self {
            item,
            name: options.name,
            crate_path: CratePath::new(options.crate_path).resolve()?,
            classes: options.classes,
            init_hook,
            members,
        })
    }

    fn validate_impl(item: &ItemImpl) -> Result<(), HostMacroError> {
        if item.trait_.is_some() {
            return Err(syn::Error::new(
                item.impl_token.span(),
                "host_module applies only to inherent implementations",
            )
            .into());
        }

        if item.unsafety.is_some() {
            return Err(syn::Error::new(
                item.unsafety.span(),
                "unsafe host module implementations are not supported",
            )
            .into());
        }

        if item.defaultness.is_some() {
            return Err(syn::Error::new(
                item.defaultness.span(),
                "specialized host module implementations are not supported",
            )
            .into());
        }

        match item.self_ty.as_ref() {
            Type::Path(path) if path.qself.is_none() => Ok(()),
            target => {
                Err(syn::Error::new_spanned(target, "host_module requires a nominal type target")
                    .into())
            }
        }
    }

    fn reject_unsupported_member(
        attrs: &mut Vec<Attribute>,
        span: Span,
    ) -> Result<(), HostMacroError> {
        if HelperAttributes::take(attrs)?.is_empty() {
            return Ok(());
        }

        Err(
            syn::Error::new(span, "this associated item cannot be exported from a host module")
                .into(),
        )
    }

    fn insert_name(
        names: &mut HashMap<String, Span>,
        name: String,
        span: Span,
        kind: &str,
    ) -> Result<(), HostMacroError> {
        let Some(previous) = names.insert(name.clone(), span) else {
            return Ok(());
        };
        let mut error =
            syn::Error::new(span, format!("duplicate guest-visible host module {kind} {name:?}"));

        error.combine(syn::Error::new(previous, format!("the first host module {kind} is here")));

        Err(error.into())
    }

    pub(crate) fn expand(self) -> TokenStream {
        let Self {
            item,
            name,
            crate_path,
            classes,
            init_hook,
            members,
        } = self;
        let target = item.self_ty.as_ref();
        let mut generics = item.generics.clone();

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#target: 'static));

        for class in classes.iter() {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(
                    #class: #crate_path::host::HostClass
                ));
        }

        for member in &members {
            member.add_predicates(&mut generics, &crate_path);
        }

        if let Some(error) = init_hook
            .as_ref()
            .and_then(|hook| hook.error())
        {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(
                    #error: Into<#crate_path::errors::Error>
                ));
        }

        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let class_registrations = classes.iter().map(|class| {
            quote! {
                exports.class::<#class>();
            }
        });
        let member_registrations = members
            .iter()
            .map(|member| member.registration(&crate_path));
        let initializer = init_hook
            .as_ref()
            .map(|hook| hook.initializer(&crate_path));

        quote! {
            #item

            impl #impl_generics #crate_path::host::HostModule for #target
                #where_clause
            {
                fn name(&self) -> &str {
                    #name
                }

                #initializer

                fn build(
                    &self,
                    exports: &mut #crate_path::host::Exports,
                ) {
                    #(#class_registrations)*
                    #(#member_registrations)*
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use crate::host::module::HostModuleMacro;

    #[test]
    fn generates_ordered_classes_and_typed_functions() {
        let output = HostModuleMacro::new(
            quote!(
                name = "@host/geometry",
                classes(Point, Shape),
                rename_all = "camelCase",
                crate_path = crate,
            ),
            parse_quote! {
                impl Geometry {
                    #[guestjs(function)]
                    fn add_values(
                        left: i32,
                        right: i32,
                    ) -> Result<i32, DomainError> {
                        Ok(left + right)
                    }

                    #[guestjs(function, name = "delayed")]
                    async fn multiply(
                        left: i32,
                        right: i32,
                    ) -> Result<i32, DomainError> {
                        Ok(left * right)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("impl crate :: host :: HostModule for Geometry"));
        assert!(output.contains("fn name (& self) -> & str"));
        assert!(output.contains("\"@host/geometry\""));
        assert!(
            output
                .find("class :: < Point >")
                .unwrap()
                < output
                    .find("class :: < Shape >")
                    .unwrap(),
        );
        assert!(
            output
                .find("class :: < Shape >")
                .unwrap()
                < output
                    .find("function (\"addValues\"")
                    .unwrap(),
        );
        assert!(output.contains("async_function (\"delayed\""));
        assert!(output.contains("await . map_err (Into :: into)"));
        assert!(output.contains("DomainError : Into < crate :: errors :: Error >"));
    }

    #[test]
    fn generates_ordered_values_and_hooks() {
        let output = HostModuleMacro::new(
            quote!(
                name = "@host/complete",
                classes(Point),
                rename_all = "camelCase",
                crate_path = crate,
            ),
            parse_quote! {
                impl Complete {
                    #[guestjs(default)]
                    const FALLBACK: &'static str = "fallback";

                    #[guestjs(function)]
                    fn read() -> Result<i32, Error> {
                        Ok(1)
                    }

                    #[guestjs(constant)]
                    const API_VERSION: i32 = 2;

                    #[guestjs(object, name = "tools")]
                    fn build_tools(
                        &self,
                        tools: &mut crate::host::Namespace,
                    ) {
                        tools.property("writable", 1);
                        tools.accessor::<_, _, _, i32>(
                            "count",
                            |_scope| Ok(1),
                            |_scope, _value| Ok(()),
                        );
                    }

                    #[guestjs(build)]
                    fn configure(
                        &self,
                        exports: &mut crate::host::Exports,
                    ) {
                        exports.constant("conditional", true);
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("exports . default (Self :: FALLBACK)"));
        assert!(output.contains("exports . constant (\"apiVersion\" , Self :: API_VERSION)"));
        assert!(output.contains("exports . object (\"tools\""));
        assert!(output.contains("Self :: build_tools (self , object)"));
        assert!(output.contains("Self :: configure (self , exports)"));
        assert!(output.contains("& 'static str : crate :: marshal :: ToGuest + Clone + 'static"));
        assert!(
            output
                .find("class :: < Point >")
                .unwrap()
                < output
                    .find("default (Self :: FALLBACK)")
                    .unwrap(),
        );
        assert!(
            output
                .find("default (Self :: FALLBACK)")
                .unwrap()
                < output
                    .find("function (\"read\"")
                    .unwrap(),
        );
        assert!(
            output
                .find("function (\"read\"")
                .unwrap()
                < output
                    .find("constant (\"apiVersion\"")
                    .unwrap(),
        );
        assert!(
            output
                .find("constant (\"apiVersion\"")
                .unwrap()
                < output
                    .find("object (\"tools\"")
                    .unwrap(),
        );
        assert!(
            output
                .find("object (\"tools\"")
                .unwrap()
                < output
                    .find("Self :: configure")
                    .unwrap(),
        );
    }

    #[test]
    fn generates_initializer_hook() {
        let output = HostModuleMacro::new(
            quote!(name = "@host/initialized", crate_path = crate),
            parse_quote! {
                impl Initialized {
                    #[guestjs(init)]
                    fn init(
                        &self,
                        scope: &crate::runtime::Scope<'_>,
                    ) -> Result<(), DomainError> {
                        scope
                            .ctx()
                            .globals()
                            .set("__initialized", true)?;

                        Ok(())
                    }

                    #[guestjs(function)]
                    fn value() -> Result<i32, DomainError> {
                        Ok(42)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("fn initialize < 'js >"));
        assert!(output.contains("scope : & crate :: runtime :: Scope < 'js >"));
        assert!(output.contains("Self :: init (self , scope) . map_err (Into :: into)"));
        assert!(output.contains("DomainError : Into < crate :: errors :: Error >"));
        assert!(output.contains("function (\"value\""));
    }

    #[test]
    fn generates_static_initializer_hook() {
        let output = HostModuleMacro::new(
            quote!(name = "@host/initialized", crate_path = crate),
            parse_quote! {
                impl Initialized {
                    #[guestjs(init)]
                    fn init(
                        scope: &crate::runtime::Scope<'_>,
                    ) -> Result<(), DomainError> {
                        scope
                            .ctx()
                            .globals()
                            .set("__initialized", true)?;

                        Ok(())
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("fn initialize < 'js >"));
        assert!(output.contains("Self :: init (scope) . map_err (Into :: into)"));
    }

    #[test]
    fn omits_initializer_hook_when_no_init_is_present() {
        let output = HostModuleMacro::new(
            quote!(name = "@host/plain", crate_path = crate),
            parse_quote! {
                impl Plain {
                    #[guestjs(function)]
                    fn value() -> Result<i32, Error> {
                        Ok(42)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(!output.contains("fn initialize < 'js >"));
    }

    #[test]
    fn preserves_outer_generics_and_unannotated_members() {
        let output = HostModuleMacro::new(
            quote!(name = "host:values", crate_path = crate),
            parse_quote! {
                impl<T> Values<T>
                where
                    T: Clone,
                {
                    #[allow(dead_code)]
                    fn helper() {}

                    #[guestjs(function)]
                    fn identity(value: T) -> Result<T, Error> {
                        Ok(value)
                    }
                }
            },
        )
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("impl < T > crate :: host :: HostModule"));
        assert!(output.contains("T : Clone"));
        assert!(output.contains("T : crate :: marshal :: FromGuestBound"));
        assert!(output.contains("T : crate :: marshal :: ToGuest"));
        assert!(output.contains("allow (dead_code)"));
        assert!(output.contains("fn helper"));
    }

    #[test]
    fn rejects_invalid_module_definitions() {
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:duplicate", classes(Point, Point), crate_path = crate),
                parse_quote!(impl Duplicate {}),
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:duplicate", classes(Point = Value), crate_path = crate),
                parse_quote!(impl Duplicate {}),
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:receiver", crate_path = crate),
                parse_quote! {
                    impl Receiver {
                        #[guestjs(function)]
                        fn read(&self) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:duplicate", crate_path = crate),
                parse_quote! {
                    impl Duplicate {
                        #[guestjs(function, name = "same")]
                        fn first() -> Result<i32, Error> {
                            Ok(1)
                        }

                        #[guestjs(function, name = "same")]
                        fn second() -> Result<i32, Error> {
                            Ok(2)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:borrow", crate_path = crate),
                parse_quote! {
                    impl Borrowing {
                        #[guestjs(function)]
                        async fn read(
                            #[guestjs(scope)] _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:bound", crate_path = crate),
                parse_quote! {
                    impl BoundValue {
                        #[guestjs(function)]
                        async fn call(
                            #[guestjs(as = crate::handle::Function)]
                            _callback: crate::handle::BoundFunction<'_>,
                        ) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:defaults", crate_path = crate),
                parse_quote! {
                    impl Defaults {
                        #[guestjs(default)]
                        const FIRST: i32 = 1;

                        #[guestjs(constant, name = "default")]
                        const SECOND: i32 = 2;
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:build", crate_path = crate),
                parse_quote! {
                    impl Builds {
                        #[guestjs(build)]
                        fn first(
                            &self,
                            _exports: &mut crate::host::Exports,
                        ) {}

                        #[guestjs(build)]
                        fn second(
                            &self,
                            _exports: &mut crate::host::Exports,
                        ) {}
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:object", crate_path = crate),
                parse_quote! {
                    impl Objects {
                        #[guestjs(object)]
                        fn invalid(
                            &mut self,
                            _object: &mut crate::host::Namespace,
                        ) {}
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:accessor", crate_path = crate),
                parse_quote! {
                    impl Accessors {
                        #[guestjs(get)]
                        fn value(&self) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        fn first(
                            &self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }

                        #[guestjs(init)]
                        fn second(
                            &self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init, name = "visible")]
                        fn init(
                            &self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        fn init(
                            &mut self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        fn init(
                            &self,
                            _exports: &mut crate::host::Exports,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        fn init(
                            &self,
                            #[guestjs(scope)] _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        fn init(
                            &self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) {}
                    }
                },
            )
            .is_err(),
        );
        assert!(
            HostModuleMacro::new(
                quote!(name = "host:init", crate_path = crate),
                parse_quote! {
                    impl Initialized {
                        #[guestjs(init)]
                        async fn init(
                            &self,
                            _scope: &crate::runtime::Scope<'_>,
                        ) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
    }
}
