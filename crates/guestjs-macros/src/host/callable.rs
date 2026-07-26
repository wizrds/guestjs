use darling::{FromMeta, util::Flag};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    AngleBracketedGenericArguments, FnArg, GenericArgument, Generics, Ident, ImplItemFn, Pat,
    PatType, Path, PathArguments, ReturnType, Type, TypeParamBound, TypePath, spanned::Spanned,
};

use crate::host::{
    HostMacroError,
    attributes::HelperAttributes,
    naming::{Naming, RenameRule},
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct MemberOptions {
    async_method: Flag,
    build: Flag,
    constructor: Flag,
    function: Flag,
    get: Flag,
    iterable: Flag,
    method: Flag,
    name: Option<String>,
    object: Flag,
    set: Flag,
    static_method: Flag,
    statics: Flag,
    symbol: Option<WellKnownSymbol>,
}

impl MemberOptions {
    fn role_count(&self) -> usize {
        [
            self.async_method.is_present(),
            self.build.is_present(),
            self.constructor.is_present(),
            self.function.is_present(),
            self.get.is_present(),
            self.iterable.is_present(),
            self.method.is_present(),
            self.object.is_present(),
            self.set.is_present(),
            self.static_method.is_present(),
            self.statics.is_present(),
            self.symbol.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }

    fn callable_kind(&self) -> Option<CallableKind> {
        if self.async_method.is_present() {
            Some(CallableKind::AsyncMethod)
        } else if self.constructor.is_present() {
            Some(CallableKind::Constructor)
        } else if self.get.is_present() {
            Some(CallableKind::Getter)
        } else if self.iterable.is_present() {
            Some(CallableKind::Iterable)
        } else if self.method.is_present() {
            Some(CallableKind::Method)
        } else if self.set.is_present() {
            Some(CallableKind::Setter)
        } else if self.static_method.is_present() {
            Some(CallableKind::StaticMethod)
        } else {
            self.symbol.map(CallableKind::Symbol)
        }
    }
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ParameterOptions {
    scope: Flag,
    borrow: Flag,
    borrow_mut: Flag,
    rest: Flag,
    #[darling(rename = "as")]
    descriptor: Option<TypePath>,
}

impl ParameterOptions {
    fn role_count(&self) -> usize {
        [
            self.scope.is_present(),
            self.borrow.is_present(),
            self.borrow_mut.is_present(),
            self.rest.is_present(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallableKind {
    AsyncMethod,
    Constructor,
    Getter,
    Iterable,
    Method,
    Setter,
    StaticMethod,
    Symbol(WellKnownSymbol),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum WellKnownSymbol {
    Iterator,
    AsyncIterator,
    ToPrimitive,
    HasInstance,
}

impl WellKnownSymbol {
    const NAMES: [&str; 4] = [
        "iterator",
        "asyncIterator",
        "toPrimitive",
        "hasInstance",
    ];

    pub(super) fn tokens(self, crate_path: &Path) -> TokenStream {
        match self {
            Self::Iterator => quote!(#crate_path::host::WellKnownSymbol::Iterator),
            Self::AsyncIterator => quote!(#crate_path::host::WellKnownSymbol::AsyncIterator),
            Self::ToPrimitive => quote!(#crate_path::host::WellKnownSymbol::ToPrimitive),
            Self::HasInstance => quote!(#crate_path::host::WellKnownSymbol::HasInstance),
        }
    }
}

impl FromMeta for WellKnownSymbol {
    fn from_string(value: &str) -> darling::Result<Self> {
        match value {
            "iterator" => Ok(Self::Iterator),
            "asyncIterator" => Ok(Self::AsyncIterator),
            "toPrimitive" => Ok(Self::ToPrimitive),
            "hasInstance" => Ok(Self::HasInstance),
            _ => Err(darling::Error::unknown_value_with_alts(
                value,
                &Self::NAMES,
            )),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Receiver {
    None,
    Shared,
    Exclusive,
}

enum ValueKind {
    Required,
    Optional,
    Nullish,
}

enum ParameterRole {
    Value {
        descriptor: Type,
        kind: ValueKind,
    },
    Borrow {
        value_type: Type,
        mutable: bool,
    },
    Rest {
        descriptor: Type,
    },
    Scope,
}

struct Parameter {
    span: Span,
    guest_index: usize,
    binding: Ident,
    materialized: Type,
    role: ParameterRole,
}

impl Parameter {
    fn new(
        argument: &mut PatType,
        guest_index: usize,
    ) -> Result<Self, HostMacroError> {
        let options = ParameterOptions::from_list(
            &HelperAttributes::take(&mut argument.attrs)?,
        )?;
        let binding = match argument.pat.as_ref() {
            Pat::Ident(binding) if binding.subpat.is_none() => binding.ident.clone(),
            pattern => {
                return Err(syn::Error::new_spanned(
                    pattern,
                    "host parameters must use identifier patterns",
                )
                .into());
            }
        };

        if options.role_count() > 1 {
            return Err(syn::Error::new(
                argument.span(),
                "a host parameter may have only one role",
            )
            .into());
        }

        let materialized = argument.ty.as_ref().clone();
        let role = if options.scope.is_present() {
            Self::scope_role(
                &materialized,
                options.descriptor.map(Type::Path),
            )?
        } else if options.borrow.is_present() {
            Self::borrow_role(
                &materialized,
                options.descriptor.map(Type::Path),
                false,
            )?
        } else if options.borrow_mut.is_present() {
            Self::borrow_role(
                &materialized,
                options.descriptor.map(Type::Path),
                true,
            )?
        } else if options.rest.is_present() {
            Self::rest_role(
                &materialized,
                options.descriptor.map(Type::Path),
            )?
        } else {
            Self::value_role(
                &materialized,
                options.descriptor.map(Type::Path),
            )
        };

        Ok(Self {
            span: argument.span(),
            guest_index,
            binding,
            materialized,
            role,
        })
    }

    fn scope_role(
        materialized: &Type,
        descriptor: Option<Type>,
    ) -> Result<ParameterRole, HostMacroError> {
        if descriptor.is_some() {
            return Err(syn::Error::new(
                materialized.span(),
                "a scope parameter cannot select a conversion descriptor",
            )
            .into());
        }

        match materialized {
            Type::Reference(reference)
                if reference.mutability.is_none()
                    && TypeShape::has_name(reference.elem.as_ref(), "Scope") =>
            {
                Ok(ParameterRole::Scope)
            }
            _ => Err(syn::Error::new(
                materialized.span(),
                "a scope parameter must have type &Scope<'_>",
            )
            .into()),
        }
    }

    fn borrow_role(
        materialized: &Type,
        descriptor: Option<Type>,
        mutable: bool,
    ) -> Result<ParameterRole, HostMacroError> {
        if descriptor.is_some() {
            return Err(syn::Error::new(
                materialized.span(),
                "a borrowed parameter cannot select a conversion descriptor",
            )
            .into());
        }

        match materialized {
            Type::Reference(reference)
                if reference.mutability.is_some() == mutable =>
            {
                Ok(ParameterRole::Borrow {
                    value_type: reference.elem.as_ref().clone(),
                    mutable,
                })
            }
            Type::Reference(_) if mutable => Err(syn::Error::new(
                materialized.span(),
                "a mutable borrow parameter must have type &mut T",
            )
            .into()),
            Type::Reference(_) => Err(syn::Error::new(
                materialized.span(),
                "a shared borrow parameter must have type &T",
            )
            .into()),
            _ => Err(syn::Error::new(
                materialized.span(),
                "a borrowed parameter must be a reference",
            )
            .into()),
        }
    }

    fn rest_role(
        materialized: &Type,
        descriptor: Option<Type>,
    ) -> Result<ParameterRole, HostMacroError> {
        match TypeShape::single_argument(materialized, "Vec") {
            Some(value_type) => Ok(ParameterRole::Rest {
                descriptor: descriptor.unwrap_or(value_type),
            }),
            None => Err(syn::Error::new(
                materialized.span(),
                "a rest parameter must have type Vec<T>",
            )
            .into()),
        }
    }

    fn value_role(
        materialized: &Type,
        descriptor: Option<Type>,
    ) -> ParameterRole {
        if let Some(value_type) = TypeShape::single_argument(materialized, "Option") {
            return ParameterRole::Value {
                descriptor: descriptor.unwrap_or(value_type),
                kind: ValueKind::Optional,
            };
        }

        if let Some(value_type) = TypeShape::single_argument(materialized, "Nullish") {
            return ParameterRole::Value {
                descriptor: descriptor.unwrap_or(value_type),
                kind: ValueKind::Nullish,
            };
        }

        ParameterRole::Value {
            descriptor: descriptor.unwrap_or_else(|| materialized.clone()),
            kind: ValueKind::Required,
        }
    }

    fn consumes_guest_argument(&self) -> bool {
        !matches!(self.role, ParameterRole::Scope)
    }

    fn is_rest(&self) -> bool {
        matches!(self.role, ParameterRole::Rest { .. })
    }

    fn is_scope(&self) -> bool {
        matches!(self.role, ParameterRole::Scope)
    }

    fn is_value(&self) -> bool {
        matches!(self.role, ParameterRole::Value { .. })
    }

    fn validate_async(&self) -> Result<(), HostMacroError> {
        match &self.role {
            ParameterRole::Borrow { .. } => Err(syn::Error::new(
                self.span,
                "an async host module function cannot retain a host-class borrow",
            )
            .into()),
            ParameterRole::Scope => Err(syn::Error::new(
                self.span,
                "an async host module function cannot retain a scope reference",
            )
            .into()),
            ParameterRole::Value { .. } | ParameterRole::Rest { .. }
                if TypeShape::has_non_static_lifetime(&self.materialized) =>
            {
                Err(syn::Error::new(
                    self.span,
                    "an async host module function cannot retain a scope-bound value",
                )
                .into())
            }
            ParameterRole::Value { .. } | ParameterRole::Rest { .. } => Ok(()),
        }
    }

    fn setter_descriptor(&self, crate_path: &Path) -> Option<TokenStream> {
        match &self.role {
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Required,
            } => Some(quote!(#descriptor)),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Optional,
            } => Some(quote!(::std::option::Option<#descriptor>)),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Nullish,
            } => Some(quote!(#crate_path::marshal::Nullish<#descriptor>)),
            ParameterRole::Borrow { .. }
            | ParameterRole::Rest { .. }
            | ParameterRole::Scope => None,
        }
    }

    fn expression(&self, crate_path: &Path) -> TokenStream {
        let index = self.guest_index;

        match &self.role {
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Required,
            } => quote!(args.get::<#descriptor>(scope, #index)?),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Optional,
            } => quote!(
                args
                    .get_opt::<::std::option::Option<#descriptor>>(
                        scope,
                        #index,
                    )?
                    .flatten()
            ),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Nullish,
            } => quote!(
                args
                    .get_opt::<#crate_path::marshal::Nullish<#descriptor>>(
                        scope,
                        #index,
                    )?
                    .unwrap_or(#crate_path::marshal::Nullish::Undefined)
            ),
            ParameterRole::Borrow {
                value_type,
                mutable: false,
            } => quote!(&*args.get_borrow::<#value_type>(scope, #index)?),
            ParameterRole::Borrow {
                value_type,
                mutable: true,
            } => quote!(&mut *args.get_borrow_mut::<#value_type>(scope, #index)?),
            ParameterRole::Rest { descriptor } => {
                quote!(args.get_rest::<#descriptor>(scope, #index)?)
            }
            ParameterRole::Scope => quote!(scope),
        }
    }

    fn accessor_expression(&self) -> TokenStream {
        match &self.role {
            ParameterRole::Value { .. } => quote!(value),
            ParameterRole::Scope => quote!(scope),
            ParameterRole::Borrow { .. } | ParameterRole::Rest { .. } => unreachable!(),
        }
    }

    fn binding(&self) -> &Ident {
        &self.binding
    }

    fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        target: &Type,
    ) {
        match &self.role {
            ParameterRole::Value { descriptor, .. }
            | ParameterRole::Rest { descriptor }
                if !TypeShape::is_target(descriptor, target) =>
            {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #descriptor: #crate_path::marshal::FromGuestBound
                    ));
            }
            ParameterRole::Borrow { value_type, .. }
                if !TypeShape::is_target(value_type, target) =>
            {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #value_type: #crate_path::host::HostClass
                    ));
            }
            ParameterRole::Value { .. }
            | ParameterRole::Borrow { .. }
            | ParameterRole::Rest { .. }
            | ParameterRole::Scope => {}
        }
    }

    fn add_module_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
    ) {
        match &self.role {
            ParameterRole::Value { descriptor, .. }
            | ParameterRole::Rest { descriptor } => {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #descriptor: #crate_path::marshal::FromGuestBound
                    ));
            }
            ParameterRole::Borrow { value_type, .. } => {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #value_type: #crate_path::host::HostClass
                    ));
            }
            ParameterRole::Scope => {}
        }
    }

    fn add_async_predicate(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
    ) {
        let materialized = &self.materialized;
        let descriptor = match &self.role {
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Required,
            } => quote!(#descriptor),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Optional,
            } => quote!(::std::option::Option<#descriptor>),
            ParameterRole::Value {
                descriptor,
                kind: ValueKind::Nullish,
            } => quote!(#crate_path::marshal::Nullish<#descriptor>),
            ParameterRole::Rest { descriptor } => {
                quote!(::std::vec::Vec<#descriptor>)
            }
            ParameterRole::Borrow { .. } | ParameterRole::Scope => return,
        };

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                for<'js> #descriptor: #crate_path::marshal::FromGuestBound<
                    Bound<'js> = #materialized,
                >
            ));
    }
}

pub(super) enum ClassMethod {
    Callable(Box<Callable>),
    Statics(StaticsHook),
}

impl ClassMethod {
    pub(super) fn new(
        method: &mut ImplItemFn,
        rename_all: Option<RenameRule>,
    ) -> Result<Option<Self>, HostMacroError> {
        let helpers = HelperAttributes::take(&mut method.attrs)?;

        if helpers.is_empty() {
            return Ok(None);
        }

        let options = MemberOptions::from_list(&helpers)?;

        if options.role_count() == 0 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "an exported host member requires a role",
            )
            .into());
        }

        if options.role_count() > 1 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a host member may have only one role",
            )
            .into());
        }

        if options.function.is_present()
            || options.object.is_present()
            || options.build.is_present()
        {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "this member role is valid only in a host module",
            )
            .into());
        }

        if options.statics.is_present() {
            return Ok(Some(Self::Statics(StaticsHook::new(
                method,
                options.name,
            )?)));
        }

        Ok(
            Some(Self::Callable(Box::new(
                Callable::new(
                    method,
                    options,
                    rename_all,
                )?,
            ))),
        )
    }
}

pub(super) enum ModuleMethod {
    Function(Box<ModuleFunction>),
    Hook(ModuleHook),
}

impl ModuleMethod {
    pub(super) fn new(
        method: &mut ImplItemFn,
        rename_all: Option<RenameRule>,
    ) -> Result<Option<Self>, HostMacroError> {
        let helpers = HelperAttributes::take(&mut method.attrs)?;

        if helpers.is_empty() {
            return Ok(None);
        }

        let options = MemberOptions::from_list(&helpers)?;

        if options.role_count() == 0 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "an exported host module member requires a role",
            )
            .into());
        }

        if options.role_count() > 1 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a host module member may have only one role",
            )
            .into());
        }

        if options.function.is_present() {
            return Ok(
                Some(Self::Function(Box::new(
                    ModuleFunction::new(
                        method,
                        options.name,
                        rename_all,
                    )?,
                ))),
            );
        }

        if options.object.is_present() {
            return Ok(Some(Self::Hook(ModuleHook::new(
                method,
                ModuleHookKind::Object,
                options.name,
                rename_all,
            )?)));
        }

        if options.build.is_present() {
            return Ok(Some(Self::Hook(ModuleHook::new(
                method,
                ModuleHookKind::Build,
                options.name,
                rename_all,
            )?)));
        }

        if options.get.is_present() || options.set.is_present() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "root host module accessors are unsupported; define them inside an object hook",
            )
            .into());
        }

        Err(syn::Error::new(
            method.sig.ident.span(),
            "this host module member role is unsupported",
        )
        .into())
    }
}

pub(super) struct ModuleFunction {
    callable: Callable,
    asynchronous: bool,
}

impl ModuleFunction {
    fn new(
        method: &mut ImplItemFn,
        name: Option<String>,
        rename_all: Option<RenameRule>,
    ) -> Result<Self, HostMacroError> {
        Ok(Self {
            asynchronous: method.sig.asyncness.is_some(),
            callable: Callable::new_module(
                method,
                name,
                rename_all,
            )?,
        })
    }

    pub(super) fn span(&self) -> Span {
        self.callable.span()
    }

    pub(super) fn name(&self) -> &str {
        self.callable.name()
    }

    pub(super) fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
    ) {
        self.callable
            .add_module_predicates(
                generics,
                crate_path,
                self.asynchronous,
            );
    }

    pub(super) fn registration(&self, crate_path: &Path) -> TokenStream {
        self.callable
            .module_registration(crate_path, self.asynchronous)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ModuleHookKind {
    Object,
    Build,
}

pub(super) struct ModuleHook {
    kind: ModuleHookKind,
    span: Span,
    ident: Ident,
    name: Option<String>,
    receiver: Receiver,
}

impl ModuleHook {
    fn new(
        method: &mut ImplItemFn,
        kind: ModuleHookKind,
        name: Option<String>,
        rename_all: Option<RenameRule>,
    ) -> Result<Self, HostMacroError> {
        Callable::validate_module_signature(method)?;

        if method.sig.asyncness.is_some() {
            return Err(syn::Error::new(
                method.sig.asyncness.span(),
                "host module hooks cannot be async",
            )
            .into());
        }

        let receiver = Callable::receiver(
            method,
            "host module hooks",
        )?;

        match (kind, receiver) {
            (ModuleHookKind::Object, Receiver::None | Receiver::Shared)
            | (ModuleHookKind::Build, Receiver::Shared) => {}
            (ModuleHookKind::Object, Receiver::Exclusive) => {
                return Err(syn::Error::new(
                    method.sig.ident.span(),
                    "an object hook cannot have an exclusive receiver",
                )
                .into());
            }
            (ModuleHookKind::Build, _) => {
                return Err(syn::Error::new(
                    method.sig.ident.span(),
                    "a build hook requires &self",
                )
                .into());
            }
        }

        if method.sig.inputs.len() != usize::from(receiver != Receiver::None) + 1 {
            return Err(syn::Error::new(
                method.sig.inputs.span(),
                "a host module hook requires exactly one builder parameter",
            )
            .into());
        }

        let Some(FnArg::Typed(argument)) = method.sig.inputs.last_mut() else {
            unreachable!();
        };

        if !HelperAttributes::take(&mut argument.attrs)?.is_empty() {
            return Err(syn::Error::new(
                argument.span(),
                "a host module hook parameter cannot have a guestjs role",
            )
            .into());
        }

        let builder = match kind {
            ModuleHookKind::Object => "Namespace",
            ModuleHookKind::Build => "Exports",
        };

        if !TypeShape::is_mutable_reference_to(argument.ty.as_ref(), builder) {
            return Err(syn::Error::new(
                argument.ty.span(),
                format!("a host module hook parameter must have type &mut {builder}"),
            )
            .into());
        }

        if !TypeShape::is_unit_return(&method.sig.output) {
            return Err(syn::Error::new(
                method.sig.output.span(),
                "a host module hook must return ()",
            )
            .into());
        }

        if kind == ModuleHookKind::Build && name.is_some() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a build hook cannot have a guest-visible name",
            )
            .into());
        }

        Ok(Self {
            kind,
            span: method.sig.ident.span(),
            ident: method.sig.ident.clone(),
            name: (kind == ModuleHookKind::Object)
                .then(|| Naming::member(&method.sig.ident, name, rename_all)),
            receiver,
        })
    }

    pub(super) fn kind(&self) -> ModuleHookKind {
        self.kind
    }

    pub(super) fn span(&self) -> Span {
        self.span
    }

    pub(super) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub(super) fn registration(&self) -> TokenStream {
        let ident = &self.ident;

        match self.kind {
            ModuleHookKind::Object => {
                let name = self.name.as_ref().unwrap();
                let receiver = match self.receiver {
                    Receiver::None => quote!(),
                    Receiver::Shared => quote!(self,),
                    Receiver::Exclusive => unreachable!(),
                };

                quote! {
                    exports.object(#name, |object| {
                        Self::#ident(#receiver object);
                    });
                }
            }
            ModuleHookKind::Build => quote! {
                Self::#ident(self, exports);
            },
        }
    }
}

pub(super) struct StaticsHook {
    span: Span,
    ident: Ident,
}

impl StaticsHook {
    fn new(
        method: &mut ImplItemFn,
        name: Option<String>,
    ) -> Result<Self, HostMacroError> {
        if name.is_some() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a statics hook cannot have a guest-visible name",
            )
            .into());
        }

        Callable::validate_common_signature(method)?;

        if method.sig.receiver().is_some() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a statics hook cannot have a receiver",
            )
            .into());
        }

        if method.sig.inputs.len() != 1 {
            return Err(syn::Error::new(
                method.sig.inputs.span(),
                "a statics hook requires one &mut Namespace parameter",
            )
            .into());
        }

        let Some(FnArg::Typed(argument)) = method.sig.inputs.first_mut() else {
            unreachable!();
        };

        if !HelperAttributes::take(&mut argument.attrs)?.is_empty() {
            return Err(syn::Error::new(
                argument.span(),
                "a statics hook parameter cannot have a guestjs role",
            )
            .into());
        }

        if !TypeShape::is_mutable_reference_to(argument.ty.as_ref(), "Namespace") {
            return Err(syn::Error::new(
                argument.ty.span(),
                "a statics hook parameter must have type &mut Namespace",
            )
            .into());
        }

        if !TypeShape::is_unit_return(&method.sig.output) {
            return Err(syn::Error::new(
                method.sig.output.span(),
                "a statics hook must return ()",
            )
            .into());
        }

        Ok(Self {
            span: method.sig.ident.span(),
            ident: method.sig.ident.clone(),
        })
    }

    pub(super) fn span(&self) -> Span {
        self.span
    }

    pub(super) fn registration(&self) -> TokenStream {
        let ident = &self.ident;

        quote! {
            Self::#ident(statics);
        }
    }

}

pub(super) struct Callable {
    kind: CallableKind,
    span: Span,
    ident: Ident,
    name: String,
    receiver: Receiver,
    parameters: Vec<Parameter>,
    result: Type,
    error: Type,
    future: Option<FutureResult>,
}

impl Callable {
    fn new(
        method: &mut ImplItemFn,
        options: MemberOptions,
        rename_all: Option<RenameRule>,
    ) -> Result<Self, HostMacroError> {
        Self::validate_common_signature(method)?;

        let kind = options.callable_kind().unwrap();
        let receiver = Self::receiver(
            method,
            "host class methods",
        )?;
        let parameters = Self::parameters(method)?;
        let (result, error) = CallableResult::parse(&method.sig.output)?;
        let future = match kind {
            CallableKind::AsyncMethod => Some(FutureResult::parse(&result)?),
            CallableKind::Constructor
            | CallableKind::Getter
            | CallableKind::Iterable
            | CallableKind::Method
            | CallableKind::Setter
            | CallableKind::StaticMethod
            | CallableKind::Symbol(_) => None,
        };
        let callable = Self {
            kind,
            span: method.sig.ident.span(),
            ident: method.sig.ident.clone(),
            name: Naming::member(&method.sig.ident, options.name, rename_all),
            receiver,
            parameters,
            result,
            error,
            future,
        };

        callable.validate_role()?;

        Ok(callable)
    }

    fn new_module(
        method: &mut ImplItemFn,
        name: Option<String>,
        rename_all: Option<RenameRule>,
    ) -> Result<Self, HostMacroError> {
        Self::validate_module_signature(method)?;

        if method.sig.receiver().is_some() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "an exported host module function cannot have a receiver",
            )
            .into());
        }

        let parameters = Self::parameters(method)?;

        if method.sig.asyncness.is_some() {
            for parameter in &parameters {
                parameter.validate_async()?;
            }
        }

        let (result, error) = CallableResult::parse(&method.sig.output)?;

        Ok(Self {
            kind: CallableKind::StaticMethod,
            span: method.sig.ident.span(),
            ident: method.sig.ident.clone(),
            name: Naming::member(&method.sig.ident, name, rename_all),
            receiver: Receiver::None,
            parameters,
            result,
            error,
            future: None,
        })
    }

    fn validate_common_signature(method: &ImplItemFn) -> Result<(), HostMacroError> {
        if method.sig.asyncness.is_some() {
            return Err(syn::Error::new(
                method.sig.asyncness.span(),
                concat!(
                    "borrowing async host class methods are unsupported; ",
                    "use a non-async async_method that returns an owned 'static future",
                ),
            )
            .into());
        }

        Self::validate_signature(method, "host class methods")
    }

    fn validate_module_signature(method: &ImplItemFn) -> Result<(), HostMacroError> {
        Self::validate_signature(method, "host module functions")
    }

    fn validate_signature(
        method: &ImplItemFn,
        kind: &str,
    ) -> Result<(), HostMacroError> {
        if method.sig.unsafety.is_some() {
            return Err(syn::Error::new(
                method.sig.unsafety.span(),
                format!("unsafe {kind} are not supported"),
            )
            .into());
        }

        if !method.sig.generics.params.is_empty() {
            return Err(syn::Error::new(
                method.sig.generics.span(),
                format!("exported {kind} cannot be generic"),
            )
            .into());
        }

        if method.sig.variadic.is_some() {
            return Err(syn::Error::new(
                method.sig.variadic.span(),
                format!("Rust variadic {kind} are not supported"),
            )
            .into());
        }

        Ok(())
    }

    fn parameters(method: &mut ImplItemFn) -> Result<Vec<Parameter>, HostMacroError> {
        let mut parameters = Vec::new();
        let mut guest_index = 0;

        for argument in &mut method.sig.inputs {
            if let FnArg::Typed(argument) = argument {
                let parameter = Parameter::new(argument, guest_index)?;

                if parameter.consumes_guest_argument() {
                    guest_index += 1;
                }

                parameters.push(parameter);
            }
        }

        let rest = parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| parameter.is_rest())
            .collect::<Vec<_>>();

        if rest.len() > 1 {
            return Err(syn::Error::new(
                rest[1].1.span,
                "a host callable may have only one rest parameter",
            )
            .into());
        }

        if rest
            .first()
            .is_some_and(|(index, _)| *index + 1 != parameters.len())
        {
            return Err(syn::Error::new(
                rest[0].1.span,
                "a rest parameter must be last",
            )
            .into());
        }

        Ok(parameters)
    }

    fn receiver(
        method: &ImplItemFn,
        subject: &str,
    ) -> Result<Receiver, HostMacroError> {
        match method.sig.receiver() {
            None => Ok(Receiver::None),
            Some(receiver) if receiver.reference.is_none() => Err(syn::Error::new_spanned(
                receiver,
                format!("{subject} require a borrowed receiver"),
            )
            .into()),
            Some(receiver) if receiver.mutability.is_some() => Ok(Receiver::Exclusive),
            Some(_) => Ok(Receiver::Shared),
        }
    }

    fn validate_role(&self) -> Result<(), HostMacroError> {
        match (self.kind, self.receiver) {
            (CallableKind::Constructor, Receiver::None)
            | (CallableKind::StaticMethod, Receiver::None)
            | (CallableKind::AsyncMethod, Receiver::Shared)
            | (CallableKind::AsyncMethod, Receiver::Exclusive)
            | (CallableKind::Getter, Receiver::Shared)
            | (CallableKind::Iterable, Receiver::Shared)
            | (CallableKind::Method, Receiver::Shared)
            | (CallableKind::Method, Receiver::Exclusive)
            | (CallableKind::Setter, Receiver::Exclusive)
            | (CallableKind::Symbol(_), Receiver::Shared) => {}
            (CallableKind::Constructor, _) => {
                return Err(syn::Error::new(
                    self.span,
                    "a host class constructor cannot have a receiver",
                )
                .into());
            }
            (CallableKind::StaticMethod, _) => {
                return Err(syn::Error::new(
                    self.span,
                    "a static host class method cannot have a receiver",
                )
                .into());
            }
            (CallableKind::Getter, _) => {
                return Err(syn::Error::new(
                    self.span,
                    "a host class getter requires &self",
                )
                .into());
            }
            (CallableKind::Setter, _) => {
                return Err(syn::Error::new(
                    self.span,
                    "a host class setter requires &mut self",
                )
                .into());
            }
            (CallableKind::Iterable, _) => {
                return Err(syn::Error::new(
                    self.span,
                    "an iterable host class method requires &self",
                )
                .into());
            }
            (CallableKind::Symbol(_), _) => {
                return Err(syn::Error::new(
                    self.span,
                    "a well-known symbol host class method requires &self",
                )
                .into());
            }
            (CallableKind::AsyncMethod, Receiver::None)
            | (CallableKind::Method, Receiver::None) => {
                return Err(syn::Error::new(
                    self.span,
                    "a host class method requires &self or &mut self",
                )
                .into());
            }
        }

        if self.kind == CallableKind::Constructor && !TypeShape::is_self(&self.result) {
            return Err(syn::Error::new(
                self.result.span(),
                "a host class constructor must return Result<Self, E>",
            )
            .into());
        }

        if matches!(self.kind, CallableKind::Getter | CallableKind::Iterable)
            && self.parameters.iter().any(Parameter::consumes_guest_argument)
        {
            return Err(syn::Error::new(
                self.span,
                "a getter or iterable method cannot accept guest values",
            )
            .into());
        }

        if self.kind == CallableKind::Setter {
            if self.parameters.iter().filter(|parameter| parameter.is_value()).count() != 1
                || self
                    .parameters
                    .iter()
                    .any(|parameter| !parameter.is_value() && !parameter.is_scope())
            {
                return Err(syn::Error::new(
                    self.span,
                    "a setter requires exactly one converted guest value",
                )
                .into());
            }

            if !TypeShape::is_unit(&self.result) {
                return Err(syn::Error::new(
                    self.result.span(),
                    "a setter must return Result<(), E>",
                )
                .into());
            }
        }

        Ok(())
    }

    fn invocation(&self, crate_path: &Path) -> TokenStream {
        let ident = &self.ident;
        let arguments = self
            .parameters
            .iter()
            .map(|parameter| parameter.expression(crate_path));

        match self.receiver {
            Receiver::None => quote!(Self::#ident(#(#arguments),*)),
            Receiver::Shared | Receiver::Exclusive => {
                quote!(Self::#ident(class #(, #arguments)*))
            }
        }
    }

    fn accessor_invocation(&self) -> TokenStream {
        let ident = &self.ident;
        let arguments = self
            .parameters
            .iter()
            .map(Parameter::accessor_expression);

        quote!(Self::#ident(class #(, #arguments)*))
    }

    fn scope_binding(&self) -> TokenStream {
        if self.parameters.is_empty() {
            quote!(_scope)
        } else {
            quote!(scope)
        }
    }

    fn args_binding(&self) -> TokenStream {
        if self
            .parameters
            .iter()
            .any(Parameter::consumes_guest_argument)
        {
            quote!(args)
        } else {
            quote!(_args)
        }
    }

    pub(super) fn kind(&self) -> CallableKind {
        self.kind
    }

    pub(super) fn span(&self) -> Span {
        self.span
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn add_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        target: &Type,
    ) {
        let error = &self.error;

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #error: Into<#crate_path::errors::Error>
            ));

        match self.kind {
            CallableKind::AsyncMethod => {
                let future = self.future.as_ref().unwrap();
                let future_error = &future.error;

                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #future_error: Into<#crate_path::errors::Error>
                    ));

                if !TypeShape::mentions_target(&future.value, target) {
                    let future_value = &future.value;

                    generics
                        .make_where_clause()
                        .predicates
                        .push(syn::parse_quote!(
                            #future_value: #crate_path::marshal::ToGuest
                        ));
                }
            }
            CallableKind::Getter
            | CallableKind::Method
            | CallableKind::StaticMethod
            | CallableKind::Symbol(_)
                if !TypeShape::mentions_target(&self.result, target) =>
            {
                let result = &self.result;

                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        #result: #crate_path::marshal::ToGuest
                    ));
            }
            CallableKind::Iterable => {
                let result = &self.result;

                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(#result: IntoIterator));
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(
                        <#result as IntoIterator>::Item: #crate_path::marshal::ToGuest
                    ));
            }
            CallableKind::Constructor
            | CallableKind::Getter
            | CallableKind::Method
            | CallableKind::Setter
            | CallableKind::StaticMethod
            | CallableKind::Symbol(_) => {}
        }

        for parameter in &self.parameters {
            parameter.add_predicates(generics, crate_path, target);
        }
    }

    pub(super) fn add_module_predicates(
        &self,
        generics: &mut Generics,
        crate_path: &Path,
        asynchronous: bool,
    ) {
        let error = &self.error;
        let result = &self.result;

        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #error: Into<#crate_path::errors::Error>
            ));
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #result: #crate_path::marshal::ToGuest
            ));

        for parameter in &self.parameters {
            parameter.add_module_predicates(generics, crate_path);

            if asynchronous {
                parameter.add_async_predicate(generics, crate_path);
            }
        }
    }

    pub(super) fn construct(&self, crate_path: &Path) -> TokenStream {
        let invocation = self.invocation(crate_path);

        quote! {
            #invocation.map_err(Into::into)
        }
    }

    pub(super) fn registration(&self, crate_path: &Path) -> TokenStream {
        let name = &self.name;
        let invocation = self.invocation(crate_path);
        let scope = self.scope_binding();
        let args = self.args_binding();

        match (self.kind, self.receiver) {
            (CallableKind::Method, Receiver::Shared) => quote! {
                spec.method(#name, |class, #scope, #args| {
                    #invocation.map_err(Into::into)
                });
            },
            (CallableKind::Method, Receiver::Exclusive) => quote! {
                spec.method_mut(#name, |class, #scope, #args| {
                    #invocation.map_err(Into::into)
                });
            },
            (CallableKind::AsyncMethod, Receiver::Shared) => {
                let future_value = &self.future.as_ref().unwrap().value;

                quote! {
                    spec.async_method(#name, |class, #scope, #args| {
                        #invocation
                            .map(|future| -> ::std::pin::Pin<
                                ::std::boxed::Box<
                                    dyn ::std::future::Future<
                                        Output = Result<
                                            #future_value,
                                            #crate_path::errors::Error,
                                        >,
                                    > + 'static,
                                >,
                            > {
                                ::std::boxed::Box::pin(async move {
                                    future.await.map_err(Into::into)
                                })
                            })
                            .map_err(Into::into)
                    });
                }
            }
            (CallableKind::AsyncMethod, Receiver::Exclusive) => {
                let future_value = &self.future.as_ref().unwrap().value;

                quote! {
                    spec.async_method_mut(#name, |class, #scope, #args| {
                        #invocation
                            .map(|future| -> ::std::pin::Pin<
                                ::std::boxed::Box<
                                    dyn ::std::future::Future<
                                        Output = Result<
                                            #future_value,
                                            #crate_path::errors::Error,
                                        >,
                                    > + 'static,
                                >,
                            > {
                                ::std::boxed::Box::pin(async move {
                                    future.await.map_err(Into::into)
                                })
                            })
                            .map_err(Into::into)
                    });
                }
            }
            (CallableKind::Symbol(symbol), Receiver::Shared) => {
                let symbol = symbol.tokens(crate_path);

                quote! {
                    spec.symbol_method(#symbol, |class, #scope, #args| {
                        #invocation.map_err(Into::into)
                    });
                }
            }
            (CallableKind::Iterable, Receiver::Shared) => quote! {
                spec.iterable(|class, #scope| {
                    #invocation.map_err(Into::into)
                });
            },
            (CallableKind::StaticMethod, Receiver::None) => quote! {
                statics.function(#name, |#scope, #args| {
                    #invocation.map_err(Into::into)
                });
            },
            _ => unreachable!(),
        }
    }

    pub(super) fn module_registration(
        &self,
        crate_path: &Path,
        asynchronous: bool,
    ) -> TokenStream {
        let name = &self.name;
        let ident = &self.ident;
        let scope = self.scope_binding();
        let args = self.args_binding();

        if !asynchronous {
            let invocation = self.invocation(crate_path);

            return quote! {
                exports.function(#name, |#scope, #args| {
                    #invocation.map_err(Into::into)
                });
            };
        }

        if self.parameters.is_empty() {
            return quote! {
                exports.async_function(#name, |#scope, #args| {
                    Ok(async move {
                        Self::#ident()
                            .await
                            .map_err(Into::into)
                    })
                });
            };
        }

        let bindings = self
            .parameters
            .iter()
            .map(Parameter::binding)
            .collect::<Vec<_>>();
        let arguments = self
            .parameters
            .iter()
            .map(|parameter| parameter.expression(crate_path));

        quote! {
            exports.async_function(#name, |#scope, #args| {
                Ok(
                    (|#(#bindings),*| async move {
                        Self::#ident(#(#bindings),*)
                            .await
                            .map_err(Into::into)
                    })(
                        #(#arguments),*
                    ),
                )
            });
        }
    }

    pub(super) fn getter_closure(&self) -> TokenStream {
        let invocation = self.accessor_invocation();
        let scope = self.scope_binding();

        quote! {
            |class, #scope| {
                #invocation.map_err(Into::into)
            }
        }
    }

    pub(super) fn setter_closure(&self) -> TokenStream {
        let invocation = self.accessor_invocation();
        let scope = if self.parameters.iter().any(Parameter::is_scope) {
            quote!(scope)
        } else {
            quote!(_scope)
        };

        quote! {
            |class, #scope, value| {
                #invocation.map_err(Into::into)
            }
        }
    }

    pub(super) fn setter_descriptor(&self, crate_path: &Path) -> TokenStream {
        self.parameters
            .iter()
            .find_map(|parameter| parameter.setter_descriptor(crate_path))
            .unwrap()
    }

    pub(super) fn uses_scope(&self) -> bool {
        !self.parameters.is_empty()
    }

    pub(super) fn uses_args(&self) -> bool {
        self.parameters
            .iter()
            .any(Parameter::consumes_guest_argument)
    }
}

struct FutureResult {
    value: Type,
    error: Type,
}

impl FutureResult {
    fn parse(future: &Type) -> Result<Self, HostMacroError> {
        let Type::ImplTrait(future) = future else {
            return Err(syn::Error::new(
                future.span(),
                concat!(
                    "an async_method must return ",
                    "Result<impl Future<Output = Result<R, E>> + 'static, E>",
                ),
            )
            .into());
        };

        if !future.bounds.iter().any(|bound| {
            matches!(
                bound,
                TypeParamBound::Lifetime(lifetime) if lifetime.ident == "static"
            )
        }) {
            return Err(syn::Error::new(
                future.span(),
                "an async_method future must be 'static",
            )
            .into());
        }

        let (value, error) = CallableResult::parse_type(
            &future
                .bounds
                .iter()
                .filter_map(|bound| match bound {
                    TypeParamBound::Trait(bound) => bound.path.segments.last(),
                    _ => None,
                })
                .filter(|segment| segment.ident == "Future")
                .find_map(|segment| match &segment.arguments {
                    PathArguments::AngleBracketed(arguments) => {
                        arguments.args.iter().find_map(|argument| match argument {
                            GenericArgument::AssocType(output) if output.ident == "Output" => {
                                Some(output.ty.clone())
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    syn::Error::new(
                        future.span(),
                        "an async_method future must declare Output = Result<R, E>",
                    )
                })?,
        )?;

        Ok(Self { value, error })
    }
}

struct CallableResult;

impl CallableResult {
    fn parse(output: &ReturnType) -> Result<(Type, Type), HostMacroError> {
        match output {
            ReturnType::Type(_, output_type) => Self::parse_type(output_type),
            ReturnType::Default => Err(syn::Error::new(
                output.span(),
                "an exported host callable must return Result<R, E>",
            )
            .into()),
        }
    }

    fn parse_type(output_type: &Type) -> Result<(Type, Type), HostMacroError> {
        let arguments = match output_type {
            Type::Path(path) => path.path.segments.last(),
            _ => None,
        }
        .filter(|segment| segment.ident == "Result")
        .and_then(|segment| match &segment.arguments {
            PathArguments::AngleBracketed(arguments) => Some(arguments),
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new(
                output_type.span(),
                "an exported host callable must return Result<R, E>",
            )
        })?;
        let types = arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                GenericArgument::Type(value_type) => Some(value_type.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        if arguments.args.len() != 2 || types.len() != 2 {
            return Err(syn::Error::new(
                output_type.span(),
                "an exported host callable must return Result<R, E>",
            )
            .into());
        }

        Ok((types[0].clone(), types[1].clone()))
    }
}

struct TypeShape;

impl TypeShape {
    fn has_non_static_lifetime(value_type: &Type) -> bool {
        match value_type {
            Type::Array(array) => Self::has_non_static_lifetime(array.elem.as_ref()),
            Type::BareFn(function) => {
                function
                    .inputs
                    .iter()
                    .any(|input| Self::has_non_static_lifetime(&input.ty))
                    || match &function.output {
                        ReturnType::Default => false,
                        ReturnType::Type(_, output) => {
                            Self::has_non_static_lifetime(output.as_ref())
                        }
                    }
            }
            Type::Group(group) => Self::has_non_static_lifetime(group.elem.as_ref()),
            Type::ImplTrait(value) => value.bounds.iter().any(|bound| {
                matches!(
                    bound,
                    TypeParamBound::Lifetime(lifetime) if lifetime.ident != "static"
                )
            }),
            Type::Paren(paren) => Self::has_non_static_lifetime(paren.elem.as_ref()),
            Type::Path(path) => path.path.segments.iter().any(|segment| {
                let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return false;
                };

                arguments.args.iter().any(|argument| match argument {
                    GenericArgument::Lifetime(lifetime) => lifetime.ident != "static",
                    GenericArgument::Type(value_type) => {
                        Self::has_non_static_lifetime(value_type)
                    }
                    GenericArgument::AssocType(binding) => {
                        Self::has_non_static_lifetime(&binding.ty)
                    }
                    GenericArgument::AssocConst(_)
                    | GenericArgument::Constraint(_)
                    | GenericArgument::Const(_) => false,
                    _ => false,
                })
            }),
            Type::Ptr(pointer) => Self::has_non_static_lifetime(pointer.elem.as_ref()),
            Type::Reference(reference) => {
                reference
                    .lifetime
                    .as_ref()
                    .is_none_or(|lifetime| lifetime.ident != "static")
                    || Self::has_non_static_lifetime(reference.elem.as_ref())
            }
            Type::Slice(slice) => Self::has_non_static_lifetime(slice.elem.as_ref()),
            Type::TraitObject(value) => value.bounds.iter().any(|bound| {
                matches!(
                    bound,
                    TypeParamBound::Lifetime(lifetime) if lifetime.ident != "static"
                )
            }),
            Type::Tuple(tuple) => tuple
                .elems
                .iter()
                .any(Self::has_non_static_lifetime),
            _ => false,
        }
    }

    fn single_argument(value_type: &Type, name: &str) -> Option<Type> {
        let segment = match value_type {
            Type::Path(path) => path.path.segments.last(),
            _ => None,
        }?;

        if segment.ident != name {
            return None;
        }

        let arguments = match &segment.arguments {
            PathArguments::AngleBracketed(arguments) => &arguments.args,
            _ => return None,
        };

        if arguments.len() != 1 {
            return None;
        }

        match arguments.first() {
            Some(GenericArgument::Type(value_type)) => Some(value_type.clone()),
            _ => None,
        }
    }

    fn has_name(value_type: &Type, name: &str) -> bool {
        match value_type {
            Type::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == name),
            _ => false,
        }
    }

    fn is_mutable_reference_to(value_type: &Type, name: &str) -> bool {
        matches!(
            value_type,
            Type::Reference(reference)
                if reference.mutability.is_some()
                    && Self::has_name(reference.elem.as_ref(), name)
        )
    }

    fn is_self(value_type: &Type) -> bool {
        Self::has_name(value_type, "Self")
    }

    fn is_target(value_type: &Type, target: &Type) -> bool {
        Self::is_self(value_type) || value_type == target
    }

    fn mentions_target(value_type: &Type, target: &Type) -> bool {
        if Self::is_target(value_type, target) {
            return true;
        }

        match value_type {
            Type::Array(array) => Self::mentions_target(array.elem.as_ref(), target),
            Type::BareFn(function) => {
                function
                    .inputs
                    .iter()
                    .any(|input| Self::mentions_target(&input.ty, target))
                    || match &function.output {
                        ReturnType::Default => false,
                        ReturnType::Type(_, output) => {
                            Self::mentions_target(output.as_ref(), target)
                        }
                    }
            }
            Type::Group(group) => Self::mentions_target(group.elem.as_ref(), target),
            Type::ImplTrait(value) => value
                .bounds
                .iter()
                .any(|bound| Self::bound_mentions_target(bound, target)),
            Type::Paren(paren) => Self::mentions_target(paren.elem.as_ref(), target),
            Type::Path(path) => {
                path.qself
                    .as_ref()
                    .is_some_and(|qself| Self::mentions_target(qself.ty.as_ref(), target))
                    || Self::path_mentions_target(&path.path, target)
            }
            Type::Ptr(pointer) => Self::mentions_target(pointer.elem.as_ref(), target),
            Type::Reference(reference) => {
                Self::mentions_target(reference.elem.as_ref(), target)
            }
            Type::Slice(slice) => Self::mentions_target(slice.elem.as_ref(), target),
            Type::TraitObject(value) => value
                .bounds
                .iter()
                .any(|bound| Self::bound_mentions_target(bound, target)),
            Type::Tuple(tuple) => tuple
                .elems
                .iter()
                .any(|element| Self::mentions_target(element, target)),
            _ => false,
        }
    }

    fn path_mentions_target(path: &Path, target: &Type) -> bool {
        path.segments.iter().any(|segment| {
            Self::path_arguments_mention_target(&segment.arguments, target)
        })
    }

    fn path_arguments_mention_target(
        arguments: &PathArguments,
        target: &Type,
    ) -> bool {
        match arguments {
            PathArguments::None => false,
            PathArguments::AngleBracketed(arguments) => {
                Self::angle_arguments_mention_target(arguments, target)
            }
            PathArguments::Parenthesized(arguments) => {
                arguments
                    .inputs
                    .iter()
                    .any(|input| Self::mentions_target(input, target))
                    || match &arguments.output {
                        ReturnType::Default => false,
                        ReturnType::Type(_, output) => {
                            Self::mentions_target(output.as_ref(), target)
                        }
                    }
            }
        }
    }

    fn angle_arguments_mention_target(
        arguments: &AngleBracketedGenericArguments,
        target: &Type,
    ) -> bool {
        arguments.args.iter().any(|argument| match argument {
            GenericArgument::Type(value_type) => {
                Self::mentions_target(value_type, target)
            }
            GenericArgument::AssocType(binding) => {
                binding.generics.as_ref().is_some_and(|arguments| {
                    Self::angle_arguments_mention_target(arguments, target)
                }) || Self::mentions_target(&binding.ty, target)
            }
            GenericArgument::AssocConst(binding) => binding
                .generics
                .as_ref()
                .is_some_and(|arguments| {
                    Self::angle_arguments_mention_target(arguments, target)
                }),
            GenericArgument::Constraint(constraint) => {
                constraint.generics.as_ref().is_some_and(|arguments| {
                    Self::angle_arguments_mention_target(arguments, target)
                }) || constraint
                    .bounds
                    .iter()
                    .any(|bound| Self::bound_mentions_target(bound, target))
            }
            GenericArgument::Const(_) | GenericArgument::Lifetime(_) => false,
            _ => false,
        })
    }

    fn bound_mentions_target(bound: &TypeParamBound, target: &Type) -> bool {
        match bound {
            TypeParamBound::Trait(bound) => {
                Self::path_mentions_target(&bound.path, target)
            }
            TypeParamBound::Lifetime(_) => false,
            _ => false,
        }
    }

    fn is_unit(value_type: &Type) -> bool {
        matches!(value_type, Type::Tuple(tuple) if tuple.elems.is_empty())
    }

    fn is_unit_return(output: &ReturnType) -> bool {
        match output {
            ReturnType::Default => true,
            ReturnType::Type(_, value_type) => Self::is_unit(value_type),
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::{Generics, ImplItemFn, Type, parse_quote};

    use crate::host::callable::{Callable, CallableKind, ClassMethod};

    struct CallableFixture;

    impl CallableFixture {
        fn parse(method: &mut ImplItemFn) -> Callable {
            match ClassMethod::new(method, None)
                .unwrap()
                .unwrap()
            {
                ClassMethod::Callable(callable) => *callable,
                ClassMethod::Statics(_) => panic!("expected a callable"),
            }
        }

        fn predicates(
            method: &mut ImplItemFn,
            target: &Type,
        ) -> String {
            let mut generics = Generics::default();

            Self::parse(method).add_predicates(
                &mut generics,
                &parse_quote!(crate),
                target,
            );

            generics.where_clause.to_token_stream().to_string()
        }
    }

    #[test]
    fn classifies_constructor_and_method_receivers() {
        let mut constructor = parse_quote! {
            #[guestjs(constructor)]
            fn new(value: i32) -> Result<Self, DomainError> {
                Ok(Self(value))
            }
        };
        let mut method = parse_quote! {
            #[guestjs(method)]
            fn add(&mut self, value: i32) -> Result<i32, DomainError> {
                Ok(value)
            }
        };

        assert_eq!(
            CallableFixture::parse(&mut constructor).kind(),
            CallableKind::Constructor,
        );
        assert_eq!(
            CallableFixture::parse(&mut method).kind(),
            CallableKind::Method,
        );
    }

    #[test]
    fn generates_every_parameter_extraction_role() {
        let mut method = parse_quote! {
            #[guestjs(method)]
            fn read(
                &self,
                required: i32,
                optional: Option<i32>,
                nullish: Nullish<i32>,
                #[guestjs(borrow)] other: &Point,
                #[guestjs(borrow_mut)] target: &mut Point,
                #[guestjs(as = Function)] callback: BoundFunction<'_>,
                #[guestjs(scope)] scope: &Scope<'_>,
                #[guestjs(rest)] rest: Vec<i32>,
            ) -> Result<i32, DomainError> {
                Ok(required)
            }
        };
        let output = CallableFixture::parse(&mut method)
            .registration(&parse_quote!(crate))
            .to_string();

        assert!(output.contains("get :: < i32 > (scope , 0"));
        assert!(
            output.contains(
                "get_opt :: < :: std :: option :: Option < i32 >> (scope , 1",
            ),
        );
        assert!(output.contains("Nullish < i32 >"));
        assert!(output.contains("get_borrow :: < Point > (scope , 3"));
        assert!(output.contains("get_borrow_mut :: < Point > (scope , 4"));
        assert!(output.contains("get :: < Function > (scope , 5"));
        assert!(output.contains("get_rest :: < i32 > (scope , 6"));
    }

    #[test]
    fn skips_only_target_referential_result_predicates() {
        let target = parse_quote!(Point);
        let mut direct = parse_quote! {
            #[guestjs(method)]
            fn children(&self) -> Result<Vec<Self>, DomainError> {
                Ok(Vec::new())
            }
        };
        let mut nested = parse_quote! {
            #[guestjs(method)]
            fn parent(&self) -> Result<Option<Vec<Self>>, DomainError> {
                Ok(None)
            }
        };
        let mut concrete = parse_quote! {
            #[guestjs(method)]
            fn wrapped(&self) -> Result<MyBox<Point>, DomainError> {
                todo!()
            }
        };
        let mut asynchronous = parse_quote! {
            #[guestjs(async_method)]
            fn descendants(
                &self,
            ) -> Result<
                impl Future<Output = Result<Vec<Self>, FutureError>> + 'static,
                DomainError,
            > {
                todo!()
            }
        };
        let mut foreign = parse_quote! {
            #[guestjs(method)]
            fn documents(&self) -> Result<Vec<Document>, DomainError> {
                Ok(Vec::new())
            }
        };

        assert!(
            !CallableFixture::predicates(
                &mut direct,
                &target,
            )
            .contains("ToGuest"),
        );
        assert!(
            !CallableFixture::predicates(
                &mut nested,
                &target,
            )
            .contains("ToGuest"),
        );
        assert!(
            !CallableFixture::predicates(
                &mut concrete,
                &target,
            )
            .contains("ToGuest"),
        );
        assert!(
            !CallableFixture::predicates(
                &mut asynchronous,
                &target,
            )
            .contains("ToGuest"),
        );
        assert!(
            CallableFixture::predicates(
                &mut foreign,
                &target,
            )
            .contains("ToGuest"),
        );
    }

    #[test]
    fn rejects_invalid_callable_signatures() {
        let cases = [
            parse_quote! {
                #[guestjs(method)]
                fn missing_result(&self) {}
            },
            parse_quote! {
                #[guestjs(method)]
                fn owned(self) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn misplaced(
                    &self,
                    #[guestjs(rest)] values: Vec<i32>,
                    value: i32,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn conflicting(
                    &self,
                    #[guestjs(scope, borrow)] value: &Scope<'_>,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn generic<T>(&self, value: T) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                async fn asynchronous(&self) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn pattern(
                    &self,
                    (left, right): (i32, i32),
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(get)]
                fn mutable_getter(&mut self) -> Result<i32, Error> {
                    Ok(1)
                }
            },
            parse_quote! {
                #[guestjs(set)]
                fn shared_setter(&self, value: i32) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(set)]
                fn returning_setter(&mut self, value: i32) -> Result<i32, Error> {
                    Ok(value)
                }
            },
            parse_quote! {
                #[guestjs(iterable)]
                fn iterable_with_value(&self, value: i32) -> Result<Vec<i32>, Error> {
                    Ok(vec![value])
                }
            },
            parse_quote! {
                #[guestjs(async_method)]
                fn borrowed_future(
                    &self,
                ) -> Result<impl Future<Output = Result<i32, Error>>, Error> {
                    Ok(async move { Ok(1) })
                }
            },
            parse_quote! {
                #[guestjs(symbol = "toPrimitive")]
                fn mutable_symbol(&mut self) -> Result<i32, Error> {
                    Ok(1)
                }
            },
            parse_quote! {
                #[guestjs(symbol = "unknown")]
                fn unknown_symbol(&self) -> Result<i32, Error> {
                    Ok(1)
                }
            },
        ];

        for mut method in cases {
            assert!(ClassMethod::new(&mut method, None).is_err());
        }
    }

    #[test]
    fn rejects_invalid_parameter_roles() {
        let cases = [
            parse_quote! {
                #[guestjs(method)]
                fn malformed_scope(
                    &self,
                    #[guestjs(scope)] scope: Scope<'_>,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn malformed_borrow(
                    &self,
                    #[guestjs(borrow)] value: Point,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn malformed_mut_borrow(
                    &self,
                    #[guestjs(borrow_mut)] value: &Point,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn descriptor_on_scope(
                    &self,
                    #[guestjs(scope, as = Function)] scope: &Scope<'_>,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
            parse_quote! {
                #[guestjs(method)]
                fn malformed_rest(
                    &self,
                    #[guestjs(rest)] values: i32,
                ) -> Result<(), Error> {
                    Ok(())
                }
            },
        ];

        for mut method in cases {
            assert!(ClassMethod::new(&mut method, None).is_err());
        }
    }

    #[test]
    fn removes_parameter_helper_attributes() {
        let mut method = parse_quote! {
            #[guestjs(method)]
            fn read(
                &self,
                #[allow(unused)]
                #[guestjs(scope)]
                scope: &Scope<'_>,
            ) -> Result<(), Error> {
                Ok(())
            }
        };

        CallableFixture::parse(&mut method);

        assert!(!method.to_token_stream().to_string().contains("guestjs"));
        assert!(method.to_token_stream().to_string().contains("allow"));
    }
}
