use darling::{
    ast::NestedMeta,
    util::Flag,
    FromMeta,
};
use proc_macro2::{
    Span,
    TokenStream,
};
use quote::quote;
use syn::{
    parse_quote,
    Attribute,
    FnArg,
    GenericParam,
    Ident,
    ImplItem,
    ImplItemFn,
    ItemImpl,
    Path,
    ReturnType,
    Safety,
    Type,
};

use crate::{
    attributes::HelperAttributes,
    backend::{
        BackendBounds,
        BackendOption,
        BackendParameter,
    },
    diagnostics::MacroError,
    host::{
        callable::Callable,
        HostSyntax,
        MemberKey,
        MemberRegistry,
        Registration,
        StateRequirements,
    },
    naming::{
        Naming,
        RenameRule,
    },
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    backend: Option<BackendOption>,
    crate_path: Option<Path>,
}

impl ClassOptions {
    fn into_parts(self) -> (
        Option<String>,
        Option<RenameRule>,
        Option<BackendOption>,
        Option<Path>,
    ) {
        (self.name, self.rename_all, self.backend, self.crate_path)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClassRole {
    Constructor,
    Method,
    AsyncMethod,
    Get,
    Set,
    StaticMethod,
    Statics,
    Constant,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassItemOptions {
    constructor: Flag,
    method: Flag,
    async_method: Flag,
    get: Flag,
    set: Flag,
    static_method: Flag,
    statics: Flag,
    constant: Flag,
    name: Option<String>,
    symbol: Option<String>,
}

impl ClassItemOptions {
    fn role(&self, ident: &Ident) -> syn::Result<ClassRole> {
        let roles = [
            (self.constructor.is_present(), ClassRole::Constructor),
            (self.method.is_present(), ClassRole::Method),
            (self.async_method.is_present(), ClassRole::AsyncMethod),
            (self.get.is_present(), ClassRole::Get),
            (self.set.is_present(), ClassRole::Set),
            (self.static_method.is_present(), ClassRole::StaticMethod),
            (self.statics.is_present(), ClassRole::Statics),
            (self.constant.is_present(), ClassRole::Constant),
        ];

        if roles.iter().filter(|(present, _)| *present).count() != 1 {
            return Err(syn::Error::new(
                ident.span(),
                "a class member requires exactly one guestjs role",
            ));
        }

        Ok(roles.into_iter().find_map(|(present, role)| {
            present.then_some(role)
        }).ok_or_else(|| syn::Error::new(
            ident.span(),
            "missing class member role",
        ))?)
    }

    fn key(
        &self,
        ident: &Ident,
        rename: Option<RenameRule>,
    ) -> syn::Result<MemberKey> {
        MemberKey::new(
            ident,
            self.name.clone(),
            self.symbol.as_deref(),
            rename,
        )
    }
}

struct NamespaceHook {
    ident: Ident,
}

impl NamespaceHook {
    fn new(method: &mut ImplItemFn) -> Result<Self, MacroError> {
        if method.sig.receiver().is_some()
            || method.sig.asyncness.is_some()
            || method.sig.abi.is_some()
            || matches!(method.sig.safety, Safety::Unsafe(_))
            || method.sig.variadic.is_some()
            || method.sig.inputs.len() != 1
            || method.sig.generics.params.iter().any(|parameter| {
                !matches!(parameter, GenericParam::Lifetime(_))
            })
        {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "statics requires one mutable namespace argument and no receiver",
            ).into());
        }

        match &method.sig.output {
            ReturnType::Default => {}
            ReturnType::Type(_, ty)
                if matches!(ty.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty()) => {}
            _ => {
                return Err(syn::Error::new_spanned(
                    &method.sig.output,
                    "statics must return ()",
                ).into());
            }
        }

        let Some(FnArg::Typed(argument)) = method.sig.inputs.first_mut()
        else {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "statics requires a namespace argument",
            ).into());
        };

        if !matches!(
            argument.ty.as_ref(),
            Type::Reference(reference) if reference.mutability.is_some()
        ) || !HelperAttributes::take(&mut argument.attrs)?.is_empty()
        {
            return Err(syn::Error::new_spanned(
                &*argument,
                "statics takes &mut NamespaceDeclaration<B> without injection",
            ).into());
        }

        Ok(Self {
            ident: method.sig.ident.clone(),
        })
    }

    fn ident(&self) -> &Ident {
        &self.ident
    }

    fn render(
        &self,
        backend: &BackendParameter,
        namespace: &Ident,
    ) -> TokenStream {
        let ident = &self.ident;
        let turbofish = backend.turbofish();

        quote!(Self::#ident #turbofish (&mut #namespace);)
    }
}

enum ClassMember {
    Callable {
        role: ClassRole,
        callable: Callable,
        key: MemberKey,
        attributes: Vec<Attribute>,
    },
    Hook {
        hook: NamespaceHook,
        attributes: Vec<Attribute>,
    },
    Constant {
        ident: Ident,
        key: MemberKey,
        attributes: Vec<Attribute>,
    },
}

impl ClassMember {
    fn ident(&self) -> &Ident {
        match self {
            Self::Callable { callable, .. } => callable.ident(),
            Self::Hook { hook, .. } => hook.ident(),
            Self::Constant { ident, .. } => ident,
        }
    }

    fn attributes(&self) -> &[Attribute] {
        match self {
            Self::Callable { attributes, .. }
            | Self::Hook { attributes, .. }
            | Self::Constant { attributes, .. } => attributes,
        }
    }

    fn state_types(&self) -> Vec<Type> {
        match self {
            Self::Callable { callable, .. } => callable.state_types(),
            _ => Vec::new(),
        }
    }

    fn is_static(&self) -> bool {
        matches!(
            self,
            Self::Hook { .. }
                | Self::Constant { .. }
                | Self::Callable {
                    role: ClassRole::StaticMethod,
                    ..
                }
        )
    }

    fn promises(&self) -> bool {
        match self {
            Self::Callable { role, callable, .. } => {
                *role == ClassRole::AsyncMethod || callable.is_async()
            }
            _ => false,
        }
    }

    fn render(
        &self,
        krate: &Path,
        backend: &BackendParameter,
        declaration: &Ident,
        namespace: &Ident,
    ) -> TokenStream {
        let attributes = HelperAttributes::conditions(self.attributes());
        let statement = match self {
            Self::Hook { hook, .. } => hook.render(backend, namespace),
            Self::Constant { ident, key, .. } => {
                let key = key.render(krate);

                quote!(#namespace.constant(#key, Self::#ident);)
            }
            Self::Callable { role, callable, key, .. } => {
                let callback = callable.class_callback(backend);
                let key = key.render(krate);

                match role {
                    ClassRole::Constructor => {
                        quote!(#declaration = #declaration.constructor(#callback);)
                    }
                    ClassRole::StaticMethod => {
                        if callable.is_async() {
                            quote!(#namespace.function_async(#key, #callback);)
                        } else {
                            quote!(#namespace.function(#key, #callback);)
                        }
                    }
                    ClassRole::Method => {
                        quote!(#declaration = #declaration.method(#key, #callback);)
                    }
                    ClassRole::AsyncMethod => {
                        quote!(#declaration = #declaration.method_async(#key, #callback);)
                    }
                    ClassRole::Get => {
                        quote!(#declaration = #declaration.getter(#key, #callback);)
                    }
                    ClassRole::Set => {
                        quote!(#declaration = #declaration.setter(#key, #callback);)
                    }
                    ClassRole::Statics | ClassRole::Constant => {
                        TokenStream::new()
                    }
                }
            }
        };

        quote! {
            #(#attributes)*
            { #statement }
        }
    }
}

struct HostClassDefinition {
    name: String,
    krate: Path,
    backend: BackendParameter,
    bounds: BackendBounds,
    members: Vec<ClassMember>,
    requirements: StateRequirements,
}

impl HostClassDefinition {
    fn new(
        args: TokenStream,
        item: &mut ItemImpl,
    ) -> Result<Self, MacroError> {
        let target = HostSyntax::target(item)?;
        let method_generics = HostSyntax::exported_generics(item);
        let (name, rename, backend, path) = ClassOptions::from_list(
            &NestedMeta::parse_meta_list(args)?,
        )?.into_parts();
        let krate = CratePath::new(path).resolve()?;
        let mut members = Vec::new();
        let mut prototype = MemberRegistry::default();
        let mut statics = MemberRegistry::default();
        let mut constructor = false;

        for member in &mut item.items {
            match member {
                ImplItem::Fn(method) => {
                    let helpers = HelperAttributes::take(&mut method.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let options = ClassItemOptions::from_list(&helpers)?;
                    let role = options.role(&method.sig.ident)?;

                    if role == ClassRole::Statics {
                        members.push(ClassMember::Hook {
                            hook: NamespaceHook::new(method)?,
                            attributes: method.attrs.clone(),
                        });

                        continue;
                    }

                    if role == ClassRole::Constant {
                        return Err(syn::Error::new_spanned(
                            &method.sig,
                            "constant requires an associated const",
                        ).into());
                    }

                    let key = options.key(&method.sig.ident, rename)?;
                    let callable = Callable::new(&mut method.sig)?;

                    callable.validate_async(false)?;

                    match role {
                        ClassRole::Constructor => {
                            callable.validate_constructor()?;

                            if constructor {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "only one constructor is permitted",
                                ).into());
                            }

                            constructor = true;
                        }
                        ClassRole::Get => callable.validate_getter()?,
                        ClassRole::Set => callable.validate_setter()?,
                        ClassRole::StaticMethod => callable.validate_static()?,
                        ClassRole::Method | ClassRole::AsyncMethod => {
                            callable.validate_instance()?;

                            if callable.is_async() {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "instance members use a synchronous function or future factory",
                                ).into());
                            }
                        }
                        _ => {}
                    }

                    if role != ClassRole::Constructor {
                        match role {
                            ClassRole::StaticMethod => &mut statics,
                            _ => &mut prototype,
                        }
                        .insert(
                            key.clone(),
                            match role {
                                ClassRole::Get => Registration::Get,
                                ClassRole::Set => Registration::Set,
                                _ => Registration::Data,
                            },
                            callable.ident(),
                        )?;
                    }

                    members.push(ClassMember::Callable {
                        role,
                        callable,
                        key,
                        attributes: method.attrs.clone(),
                    });
                }
                ImplItem::Const(constant) => {
                    let helpers = HelperAttributes::take(&mut constant.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let options = ClassItemOptions::from_list(&helpers)?;

                    if options.role(&constant.ident)? != ClassRole::Constant {
                        return Err(syn::Error::new_spanned(
                            &*constant,
                            "an exported const requires the constant role",
                        ).into());
                    }

                    let key = options.key(&constant.ident, rename)?;

                    statics.insert(
                        key.clone(),
                        Registration::Data,
                        &constant.ident,
                    )?;

                    members.push(ClassMember::Constant {
                        ident: constant.ident.clone(),
                        key,
                        attributes: constant.attrs.clone(),
                    });
                }
                _ => {}
            }
        }

        let backend = BackendParameter::resolve(
            backend,
            &item.generics,
            &method_generics,
        )?;
        let mut bounds = BackendBounds::new(&backend, vec![
            parse_quote!(#krate::backend::classes::BackendClasses),
            parse_quote!(#krate::backend::exceptions::BackendExceptions),
        ]);

        if members.iter().any(ClassMember::promises) {
            bounds.push(parse_quote!(
                #krate::backend::promises::BackendPromises
            ));
        }

        HostSyntax::rewrite(
            item,
            &members.iter().map(|member| member.ident().clone())
                .collect::<Vec<_>>(),
            &backend,
            &mut bounds,
            &krate,
        );

        let mut requirements = StateRequirements::default();

        for member in &members {
            requirements.extend(member.state_types());
        }

        Ok(Self {
            name: Naming::member(&target, name, None),
            krate,
            backend,
            bounds,
            members,
            requirements,
        })
    }

    fn render(&self, item: &ItemImpl) -> TokenStream {
        let krate = &self.krate;
        let name = &self.name;
        let target = item.self_ty.as_ref();
        let backend = self.backend.ty();
        let payload_generics = HostSyntax::payload_generics(item);
        let (payload_impl, _, payload_where) =
            payload_generics.split_for_impl();
        let class_generics = self.backend.definition_generics(
            &payload_generics,
            &self.bounds,
        );
        let (class_impl, _, class_where) = class_generics.split_for_impl();
        let conditions = HelperAttributes::conditions(&item.attrs);
        let declaration = Ident::new("__guestjs_declaration", Span::mixed_site());
        let namespace = Ident::new("__guestjs_namespace", Span::mixed_site());
        let requirements = self.requirements.render();
        let mutable = self.members.iter()
            .any(|member| !member.is_static())
            .then(|| quote!(mut));
        let has_statics = self.members.iter().any(ClassMember::is_static);
        let namespace_setup = has_statics.then(|| quote! {
            let mut #namespace = #krate::host::namespace::NamespaceDeclaration::new();
        });
        let prototype = self.members.iter()
            .filter(|member| !member.is_static())
            .map(|member| member.render(
                krate, &self.backend, &declaration, &namespace,
            ));
        let statics = self.members.iter()
            .filter(|member| member.is_static())
            .map(|member| member.render(
                krate, &self.backend, &declaration, &namespace,
            ));
        let result = if has_statics {
            quote!(#declaration.statics(#namespace))
        } else {
            quote!(#declaration)
        };

        quote! {
            #(#conditions)*
            impl #payload_impl #krate::handle::class::Payload
                for #target #payload_where
            {
                const NAME: &'static str = #name;
            }

            #(#conditions)*
            impl #class_impl #krate::host::class::HostClass<#backend>
                for #target #class_where
            {
                fn declaration()
                    -> #krate::host::class::ClassDeclaration<#backend, Self>
                {
                    let #mutable #declaration =
                        #krate::host::class::ClassDeclaration::new()
                            #(#requirements)*;

                    #(#prototype)*
                    #namespace_setup
                    #(#statics)*

                    #result
                }
            }
        }
    }
}

pub struct HostClassMacro {
    item: ItemImpl,
    definition: HostClassDefinition,
}

impl HostClassMacro {
    pub fn new(
        args: TokenStream,
        mut item: ItemImpl,
    ) -> Result<Self, MacroError> {
        Ok(Self {
            definition: HostClassDefinition::new(args, &mut item)?,
            item,
        })
    }

    pub fn expand(self) -> TokenStream {
        let definition = self.definition.render(&self.item);
        let item = self.item;

        quote! {
            #item
            #definition
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{
        parse2,
        parse_quote,
        File,
    };

    use crate::host::class::HostClassMacro;

    #[test]
    fn preserves_source_and_emits_payload_and_class_impls() {
        assert_eq!(
            parse2::<File>(
                HostClassMacro::new(
                    quote!(crate_path = crate),
                    parse_quote!(impl Counter {
                        fn internal() {}

                        #[guestjs(constructor)]
                        fn new() -> Result<Self, Error> {
                            Ok(Self)
                        }
                    }),
                )
                .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
                .expand(),
            )
            .unwrap()
            .items.len(),
            3,
        );
    }

    #[test]
    fn accepts_annotated_constants_and_mutable_unit_statics_hooks() {
        assert!(HostClassMacro::new(
            quote!(crate_path = crate, backend = B),
            parse_quote!(impl Counter {
                const RUST_ONLY: u8 = 1;

                #[guestjs(constant)]
                const CATEGORY: &'static str = "counter";

                #[guestjs(statics)]
                fn configure(namespace: &mut NamespaceDeclaration<B>) {
                    namespace.constant("version", "1");
                }
            }),
        ).is_ok());
    }

    #[test]
    fn rejects_duplicate_keys_and_owned_statics_hooks() {
        assert!(HostClassMacro::new(
            quote!(crate_path = crate),
            parse_quote!(impl Counter {
                #[guestjs(method)]
                fn read(&self) -> Result<(), Error> { Ok(()) }

                #[guestjs(get, name = "read")]
                fn other(&self) -> Result<(), Error> { Ok(()) }
            }),
        ).is_err());
        assert!(HostClassMacro::new(
            quote!(crate_path = crate),
            parse_quote!(impl Counter {
                #[guestjs(statics)]
                fn configure(namespace: NamespaceDeclaration<B>)
                    -> NamespaceDeclaration<B>
                {
                    namespace
                }
            }),
        ).is_err());
    }
}
