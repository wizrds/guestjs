use darling::{FromMeta, ast::NestedMeta, util::Flag};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Attribute, Ident, ImplItem, ItemImpl, Path, Type, parse_quote};

use crate::{
    attributes::HelperAttributes,
    backend::{BackendBounds, BackendOption, BackendParameter},
    diagnostics::MacroError,
    host::{HostSyntax, StateRequirements, TypeList, callable::Callable, receiver::ModuleReceiver},
    naming::{Naming, RenameRule},
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ModuleOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    method_name: Option<Ident>,
    backend: Option<BackendOption>,
    classes: TypeList,
    exceptions: TypeList,
    crate_path: Option<Path>,
}

impl ModuleOptions {
    fn into_parts(
        self,
    ) -> (
        Option<String>,
        Option<RenameRule>,
        Option<Ident>,
        Option<BackendOption>,
        TypeList,
        TypeList,
        Option<Path>,
    ) {
        (
            self.name,
            self.rename_all,
            self.method_name,
            self.backend,
            self.classes,
            self.exceptions,
            self.crate_path,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModuleRole {
    Function,
    AsyncFunction,
    Constant,
    Namespace,
    State,
    Init,
    Build,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ModuleItemOptions {
    function: Flag,
    async_function: Flag,
    constant: Flag,
    namespace: Flag,
    state: Flag,
    init: Flag,
    build: Flag,
    default: Flag,
    name: Option<String>,
}

impl ModuleItemOptions {
    fn resolve(
        &self,
        ident: &Ident,
        rename: Option<RenameRule>,
    ) -> syn::Result<(ModuleRole, String)> {
        let roles = [
            (self.function.is_present(), ModuleRole::Function),
            (self.async_function.is_present(), ModuleRole::AsyncFunction),
            (self.constant.is_present(), ModuleRole::Constant),
            (self.namespace.is_present(), ModuleRole::Namespace),
            (self.state.is_present(), ModuleRole::State),
            (self.init.is_present(), ModuleRole::Init),
            (self.build.is_present(), ModuleRole::Build),
        ];

        if roles
            .iter()
            .filter(|(present, _)| *present)
            .count()
            != 1
        {
            return Err(syn::Error::new(
                ident.span(),
                "a module member requires exactly one guestjs role",
            ));
        }

        let role = roles
            .into_iter()
            .find_map(|(present, role)| present.then_some(role))
            .ok_or_else(|| syn::Error::new(ident.span(), "missing module member role"))?;

        if self.default.is_present()
            && matches!(
                role,
                ModuleRole::State | ModuleRole::Init | ModuleRole::Build
            )
        {
            return Err(syn::Error::new(
                ident.span(),
                "only an export can be the default export",
            ));
        }

        if self.default.is_present() && self.name.is_some() {
            return Err(syn::Error::new(
                ident.span(),
                "a default export cannot have a guest-visible name",
            ));
        }

        Ok((
            role,
            match self.default.is_present() {
                true => String::from("default"),
                false => Naming::member(ident, self.name.clone(), rename),
            },
        ))
    }
}

enum ModuleMember {
    Function {
        callable: Callable,
        receiver: ModuleReceiver,
        asynchronous: bool,
        name: String,
        attributes: Vec<Attribute>,
    },
    Namespace {
        callable: Callable,
        name: String,
        attributes: Vec<Attribute>,
    },
    Constant {
        ident: Ident,
        name: String,
        attributes: Vec<Attribute>,
    },
}

impl ModuleMember {
    fn ident(&self) -> &Ident {
        match self {
            Self::Function { callable, .. } | Self::Namespace { callable, .. } => callable.ident(),
            Self::Constant { ident, .. } => ident,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Function { name, .. }
            | Self::Namespace { name, .. }
            | Self::Constant { name, .. } => name,
        }
    }

    fn shared(&self) -> bool {
        matches!(
            self,
            Self::Function { receiver, .. } if receiver.requires_state()
        )
    }

    fn promises(&self) -> bool {
        matches!(self, Self::Function { asynchronous: true, .. })
    }

    fn requirements(&self, krate: &Path) -> Vec<Type> {
        match self {
            Self::Function { callable, receiver, .. } => {
                let mut types = callable.state_types();

                if receiver.requires_state() {
                    types.push(parse_quote!(
                        #krate::host::state::ModuleState<Self>
                    ));
                }

                types
            }
            _ => Vec::new(),
        }
    }

    fn render(&self, krate: &Path, backend: &BackendParameter, declaration: &Ident) -> TokenStream {
        let (attributes, operation) = match self {
            Self::Constant { ident, name, attributes } => {
                (attributes, quote!(.value(#name, Self::#ident)))
            }
            Self::Namespace { callable, name, attributes } => {
                let invocation = callable.associated(backend);

                (attributes, quote!(.namespace(#name, #invocation)))
            }
            Self::Function {
                callable,
                receiver,
                asynchronous,
                name,
                attributes,
            } => {
                let callback = callable.module_callback(krate, backend, receiver);

                (
                    attributes,
                    if *asynchronous {
                        quote!(.function_async(#name, #callback))
                    } else {
                        quote!(.function(#name, #callback))
                    },
                )
            }
        };
        let conditions = HelperAttributes::conditions(attributes);

        quote! {
            #(#conditions)*
            { #declaration = #declaration #operation; }
        }
    }
}

struct HostModuleDefinition {
    name: String,
    method_name: Ident,
    krate: Path,
    backend: BackendParameter,
    bounds: BackendBounds,
    members: Vec<ModuleMember>,
    factory: Option<Callable>,
    initializer: Option<Callable>,
    build: Option<Callable>,
    shared: bool,
    requirements: StateRequirements,
    classes: TypeList,
    exceptions: TypeList,
}

impl HostModuleDefinition {
    fn new(args: TokenStream, item: &mut ItemImpl) -> Result<Self, MacroError> {
        let target = HostSyntax::target(item)?;
        let exported_generics = HostSyntax::exported_generics(item);
        let (name, rename, method_name, backend, classes, exceptions, path) =
            ModuleOptions::from_list(&NestedMeta::parse_meta_list(args)?)?.into_parts();
        let krate = CratePath::new(path).resolve()?;
        let mut members = Vec::new();
        let mut factory = None;
        let mut initializer = None;
        let mut build = None;

        for member in &mut item.items {
            match member {
                ImplItem::Fn(method) => {
                    let helpers = HelperAttributes::take(&mut method.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let (role, name) = ModuleItemOptions::from_list(&helpers)?
                        .resolve(&method.sig.ident, rename)?;
                    let receiver = ModuleReceiver::new(&method.sig)?;
                    let callable = Callable::new(&mut method.sig)?;

                    callable.validate_async(true)?;

                    match role {
                        ModuleRole::State => {
                            callable.validate_factory("a state factory")?;

                            if factory.is_some() {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "only one state factory is permitted",
                                )
                                .into());
                            }

                            factory = Some(callable);
                        }
                        ModuleRole::Init => {
                            callable.validate_factory("an init hook")?;

                            if initializer.is_some() {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "only one init hook is permitted",
                                )
                                .into());
                            }

                            initializer = Some(callable);
                        }
                        ModuleRole::Build => {
                            callable.validate_build_hook()?;

                            if build.is_some() {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "only one build hook is permitted",
                                )
                                .into());
                            }

                            build = Some(callable);
                        }
                        ModuleRole::Namespace => {
                            callable.validate_namespace_factory()?;

                            members.push(ModuleMember::Namespace {
                                callable,
                                name,
                                attributes: method.attrs.clone(),
                            });
                        }
                        ModuleRole::Function | ModuleRole::AsyncFunction => {
                            if role == ModuleRole::AsyncFunction && callable.is_async() {
                                return Err(syn::Error::new(
                                    callable.ident().span(),
                                    "async_function marks a synchronous future factory",
                                )
                                .into());
                            }

                            members.push(ModuleMember::Function {
                                asynchronous: callable.is_async()
                                    || role == ModuleRole::AsyncFunction,
                                callable,
                                receiver,
                                name,
                                attributes: method.attrs.clone(),
                            });
                        }
                        ModuleRole::Constant => {
                            return Err(syn::Error::new(
                                callable.ident().span(),
                                "constant requires an associated const",
                            )
                            .into());
                        }
                    }
                }
                ImplItem::Const(constant) => {
                    let helpers = HelperAttributes::take(&mut constant.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let (role, name) =
                        ModuleItemOptions::from_list(&helpers)?.resolve(&constant.ident, rename)?;

                    if role != ModuleRole::Constant {
                        return Err(syn::Error::new(
                            constant.ident.span(),
                            "an exported const requires the constant role",
                        )
                        .into());
                    }

                    members.push(ModuleMember::Constant {
                        ident: constant.ident.clone(),
                        name,
                        attributes: constant.attrs.clone(),
                    });
                }
                _ => {}
            }
        }

        let mut names = Vec::new();

        for member in &members {
            if names.contains(&member.name()) {
                return Err(
                    syn::Error::new(member.ident().span(), "duplicate module export").into()
                );
            }

            names.push(member.name());
        }

        let backend = BackendParameter::resolve(backend, &item.generics, &exported_generics)?;
        let mut bounds = BackendBounds::new(&backend, vec![parse_quote!(#krate::backend::Backend)]);

        if members.iter().any(|member| {
            matches!(member, ModuleMember::Function { .. } | ModuleMember::Namespace { .. })
        }) {
            bounds.push(parse_quote!(
                #krate::backend::callables::BackendCallables
            ));
        }

        if members
            .iter()
            .any(ModuleMember::promises)
        {
            bounds.push(parse_quote!(
                #krate::backend::promises::BackendPromises
            ));
            bounds.push(parse_quote!(
                #krate::backend::exceptions::BackendExceptions
            ));
        }

        if !classes.types().is_empty() || !exceptions.types().is_empty() {
            bounds.push(parse_quote!(
                #krate::backend::classes::BackendClasses
            ));
            bounds.push(parse_quote!(
                #krate::backend::exceptions::BackendExceptions
            ));
        }

        HostSyntax::rewrite(
            item,
            &members
                .iter()
                .map(ModuleMember::ident)
                .chain(
                    factory
                        .iter()
                        .chain(&initializer)
                        .chain(&build)
                        .map(Callable::ident),
                )
                .cloned()
                .collect::<Vec<_>>(),
            &backend,
            &mut bounds,
            &krate,
        );

        let mut requirements = StateRequirements::default();

        for member in &members {
            requirements.extend(member.requirements(&krate));
        }

        Ok(Self {
            name: Naming::member(&target, name, None),
            method_name: method_name.unwrap_or_else(|| parse_quote!(module)),
            krate,
            backend,
            bounds,
            shared: members.iter().any(ModuleMember::shared),
            members,
            factory,
            initializer,
            build,
            requirements,
            classes,
            exceptions,
        })
    }

    fn render(&self, item: &ItemImpl) -> TokenStream {
        let target = item.self_ty.as_ref();
        let (source_impl, _, source_where) = item.generics.split_for_impl();
        let mut generics = self
            .backend
            .definition_generics(&item.generics, &self.bounds);

        if self.shared || self.factory.is_some() || self.initializer.is_some() {
            generics
                .make_where_clause()
                .predicates
                .push(match self.shared && self.factory.is_none() {
                    true => parse_quote!(Self: Default + 'static),
                    false => parse_quote!(Self: 'static),
                });
        }

        let method_where = &generics.where_clause;
        let method_generics = self.backend.method_generics();
        let krate = &self.krate;
        let backend = self.backend.ty();
        let name = &self.name;
        let method_name = &self.method_name;
        let conditions = HelperAttributes::conditions(&item.attrs);
        let requirements = self.requirements.render();
        let declaration = Ident::new("__guestjs_module", Span::mixed_site());
        let mutable = (!self.members.is_empty()).then(|| quote!(mut));
        let factory = match &self.factory {
            Some(factory) => {
                let factory = factory.factory(&self.backend);

                quote!(.state(#factory))
            }
            None if self.shared => {
                quote!(.state(|_| ::core::result::Result::Ok(Self::default())))
            }
            None => TokenStream::new(),
        };
        let initializer = self
            .initializer
            .as_ref()
            .map(|initializer| {
                let initializer = initializer.factory(&self.backend);

                quote!(.initialize(#initializer))
            });
        let members = self
            .members
            .iter()
            .map(|member| member.render(krate, &self.backend, &declaration));
        let classes = self.classes.types();
        let exceptions = self.exceptions.types();
        let registered = quote! {
            #declaration
                #(.exception::<#exceptions>())*
                #(.class::<#classes>())*
        };
        let finished = match &self.build {
            Some(build) => {
                let ident = build.ident();
                let turbofish = self.backend.turbofish();

                quote!(Self::#ident #turbofish (#registered))
            }
            None => registered,
        };

        quote! {
            #(#conditions)*
            impl #source_impl #target #source_where {
                pub fn #method_name #method_generics ()
                    -> #krate::host::module::ModuleDeclaration<#backend>
                #method_where
                {
                    let #mutable #declaration =
                        #krate::host::module::ModuleDeclaration::new(#name)
                            #factory
                            #initializer
                            #(#requirements)*;

                    #(#members)*

                    #finished
                }
            }
        }
    }
}

pub struct HostModuleMacro {
    item: ItemImpl,
    definition: HostModuleDefinition,
}

impl HostModuleMacro {
    pub fn new(args: TokenStream, mut item: ItemImpl) -> Result<Self, MacroError> {
        Ok(Self {
            definition: HostModuleDefinition::new(args, &mut item)?,
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
    use proc_macro2::TokenStream;
    use quote::quote;
    use syn::{File, ItemImpl, parse_quote, parse2};

    use crate::host::module::HostModuleMacro;

    struct ModuleFixture;

    impl ModuleFixture {
        fn emits(item: ItemImpl, expected: TokenStream) -> bool {
            HostModuleMacro::new(quote!(crate_path = crate), item)
                .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
                .expand()
                .to_string()
                .contains(&expected.to_string())
        }

        fn rejects(item: ItemImpl) -> bool {
            HostModuleMacro::new(quote!(crate_path = crate), item).is_err()
        }
    }

    #[test]
    fn emits_a_stateful_module_and_preserves_the_original_impl() {
        assert_eq!(
            parse2::<File>(
                HostModuleMacro::new(
                    quote!(name = "metrics", crate_path = crate),
                    parse_quote!(impl Metrics {
                        #[guestjs(function)]
                        fn increment(&self, amount: f64)
                            -> Result<f64, Error>
                        {
                            Ok(amount)
                        }
                    }),
                )
                .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
                .expand(),
            )
            .unwrap()
            .items
            .len(),
            2,
        );
    }

    #[test]
    fn rejects_exclusive_module_receivers() {
        assert!(
            HostModuleMacro::new(
                quote!(crate_path = crate),
                parse_quote!(impl Metrics {
                    #[guestjs(function)]
                    fn increment(&mut self) -> Result<(), Error> { Ok(()) }
                }),
            )
            .is_err()
        );
    }

    #[test]
    fn default_exports_under_the_default_name() {
        assert!(ModuleFixture::emits(
            parse_quote!(impl Greeting {
                #[guestjs(constant, default)]
                const FALLBACK: &'static str = "hello";
            }),
            quote!(.value("default", Self::FALLBACK)),
        ));
    }

    #[test]
    fn rejects_misused_defaults() {
        for item in [
            parse_quote!(impl Greeting {
                #[guestjs(constant, default, name = "fallback")]
                const FALLBACK: &'static str = "hello";
            }),
            parse_quote!(impl Greeting {
                #[guestjs(state, default)]
                fn state() -> Result<Self, Error> { Ok(Self) }
            }),
            parse_quote!(impl Greeting {
                #[guestjs(init, default)]
                fn install() -> Result<(), Error> { Ok(()) }
            }),
            parse_quote!(impl Greeting {
                #[guestjs(build, default)]
                fn configure(module: ModuleDeclaration<B>)
                    -> ModuleDeclaration<B>
                {
                    module
                }
            }),
            parse_quote!(impl Greeting {
                #[guestjs(constant, default)]
                const FALLBACK: &'static str = "hello";

                #[guestjs(function, name = "default")]
                fn greet() -> Result<(), Error> { Ok(()) }
            }),
        ] {
            assert!(ModuleFixture::rejects(item));
        }
    }

    #[test]
    fn init_renders_an_initializer_and_a_static_bound() {
        for expected in [
            quote!(.initialize(|__guestjs_enter| Self::install())),
            quote!(Self: 'static),
        ] {
            assert!(ModuleFixture::emits(
                parse_quote!(impl Geometry {
                    #[guestjs(init)]
                    fn install() -> Result<(), Error> { Ok(()) }
                }),
                expected,
            ));
        }
    }

    #[test]
    fn rejects_misplaced_init_hooks() {
        for item in [
            parse_quote!(impl Geometry {
                #[guestjs(init)]
                fn first() -> Result<(), Error> { Ok(()) }

                #[guestjs(init)]
                fn second() -> Result<(), Error> { Ok(()) }
            }),
            parse_quote!(impl Geometry {
                #[guestjs(init)]
                fn install(&self) -> Result<(), Error> { Ok(()) }
            }),
            parse_quote!(impl Geometry {
                #[guestjs(init)]
                fn install(scale: f64) -> Result<(), Error> { Ok(()) }
            }),
            parse_quote!(impl Geometry {
                #[guestjs(init)]
                async fn install() -> Result<(), Error> { Ok(()) }
            }),
        ] {
            assert!(ModuleFixture::rejects(item));
        }
    }

    #[test]
    fn build_wraps_the_finished_declaration() {
        assert!(
            HostModuleMacro::new(
                quote!(backend = B, classes(Point), crate_path = crate),
                parse_quote!(impl Geometry {
                    #[guestjs(build)]
                    fn configure(module: ModuleDeclaration<B>)
                        -> ModuleDeclaration<B>
                    {
                        module
                    }
                }),
            )
            .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
            .expand()
            .to_string()
            .contains(
                &quote!(Self::configure::<B>(__guestjs_module .class::<Point>()))
                    .to_string()
            )
        );
    }

    #[test]
    fn rejects_misplaced_build_hooks() {
        for item in [
            parse_quote!(impl Geometry {
                #[guestjs(build)]
                fn first(module: ModuleDeclaration<B>) -> ModuleDeclaration<B> { module }

                #[guestjs(build)]
                fn second(module: ModuleDeclaration<B>) -> ModuleDeclaration<B> { module }
            }),
            parse_quote!(impl Geometry {
                #[guestjs(build)]
                fn configure(&self, module: ModuleDeclaration<B>)
                    -> ModuleDeclaration<B>
                {
                    module
                }
            }),
            parse_quote!(impl Geometry {
                #[guestjs(build)]
                fn configure() -> ModuleDeclaration<B> { ModuleDeclaration::new("geometry") }
            }),
        ] {
            assert!(ModuleFixture::rejects(item));
        }
    }

    #[test]
    fn namespace_renders_its_factory() {
        assert!(ModuleFixture::emits(
            parse_quote!(impl Geometry {
                #[guestjs(namespace)]
                fn tools() -> NamespaceDeclaration<B> {
                    NamespaceDeclaration::new()
                        .constant("version", 2)
                        .clone()
                }
            }),
            quote!(.namespace("tools", Self::tools())),
        ));
    }

    #[test]
    fn state_renders_its_factory_and_a_static_bound() {
        for expected in [
            quote!(.state(|__guestjs_enter| Self::state())),
            quote!(Self: 'static),
        ] {
            assert!(ModuleFixture::emits(
                parse_quote!(impl Geometry {
                    #[guestjs(state)]
                    fn state() -> Result<Self, Error> { Ok(Self) }
                }),
                expected,
            ));
        }
    }
}
