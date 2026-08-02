use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, hash_map::Entry},
    rc::Rc,
    sync::Arc,
};

use rquickjs::{
    Ctx, Error as JsError, JsLifetime, Module as JsModule, Result as JsResult,
    loader::{ImportAttributes, Loader, Resolver},
};

use crate::{
    errors::Error,
    host::{HostLibrary, HostModule, HostModuleAdapter, Namespace},
    native::{NativeInitializer, NativeLibrary, NativeLibraryEntry, NativeModule},
};

#[derive(Clone)]
pub(crate) enum LibraryBinding {
    Host(Arc<dyn HostModule>),
    Native(NativeModule),
    Initializer(NativeInitializer),
}

impl LibraryBinding {
    pub(crate) fn from_host(library: HostLibrary) -> Vec<Self> {
        library
            .into_modules()
            .into_iter()
            .map(Self::Host)
            .collect()
    }

    pub(crate) fn from_native(library: NativeLibrary) -> Vec<Self> {
        library
            .into_entries()
            .into_iter()
            .map(|entry| match entry {
                NativeLibraryEntry::Module(module) => Self::Native(module),
                NativeLibraryEntry::Initializer(initializer) => Self::Initializer(initializer),
            })
            .collect()
    }
}

#[derive(Clone)]
enum ModuleRegistration {
    Host(Arc<dyn HostModule>),
    Native(NativeModule),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GuestId(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ContextKey(usize);

impl ContextKey {
    fn new(ctx: &Ctx<'_>) -> Self {
        Self(ctx.as_raw().as_ptr() as usize)
    }
}

pub(crate) struct GuestRegistration {
    id: GuestId,
    initializers: Vec<NativeInitializer>,
}

impl GuestRegistration {
    pub(crate) fn id(&self) -> GuestId {
        self.id
    }

    pub(crate) fn into_initializers(self) -> Vec<NativeInitializer> {
        self.initializers
    }
}

struct GuestRegistry {
    context: ContextKey,
    specifiers: HashMap<String, String>,
    modules: HashMap<String, ModuleRegistration>,
    staged: HashMap<String, Namespace>,
}

impl GuestRegistry {
    fn new(
        context: ContextKey,
        guest: GuestId,
        bindings: &[LibraryBinding],
    ) -> (Self, Vec<NativeInitializer>) {
        let mut specifier_ordinals = HashMap::new();

        for (ordinal, binding) in bindings.iter().enumerate() {
            match binding {
                LibraryBinding::Host(module) => {
                    specifier_ordinals.insert(module.name().to_owned(), ordinal);
                }
                LibraryBinding::Native(module) => {
                    specifier_ordinals.insert(module.name().to_owned(), ordinal);

                    for alias in module.aliases() {
                        specifier_ordinals.insert(alias.clone(), ordinal);
                    }
                }
                LibraryBinding::Initializer(_) => {}
            }
        }

        let live_ordinals = specifier_ordinals
            .values()
            .copied()
            .collect::<HashSet<_>>();

        let mut routes = HashMap::new();
        let mut modules = HashMap::new();

        for (ordinal, binding) in bindings.iter().enumerate() {
            if !live_ordinals.contains(&ordinal) {
                continue;
            }

            let route = format!("guestjs:module:{}:{ordinal}", guest.0);

            routes.insert(ordinal, route.clone());
            modules.insert(
                route,
                match binding {
                    LibraryBinding::Host(module) => ModuleRegistration::Host(module.clone()),
                    LibraryBinding::Native(module) => ModuleRegistration::Native(module.clone()),
                    LibraryBinding::Initializer(_) => continue,
                },
            );
        }

        let mut initializer_names = HashSet::new();
        let mut initializers = Vec::new();

        for (ordinal, binding) in bindings.iter().enumerate().rev() {
            match binding {
                LibraryBinding::Native(module) if live_ordinals.contains(&ordinal) => {
                    for initializer in module.initializers().iter().rev() {
                        if initializer_names.insert(initializer.name().to_owned()) {
                            initializers.push(initializer.clone());
                        }
                    }
                }
                LibraryBinding::Initializer(initializer)
                    if initializer_names.insert(initializer.name().to_owned()) =>
                {
                    initializers.push(initializer.clone());
                }
                LibraryBinding::Host(_)
                | LibraryBinding::Native(_)
                | LibraryBinding::Initializer(_) => {}
            }
        }

        initializers.reverse();

        (
            Self {
                context,
                specifiers: specifier_ordinals
                    .into_iter()
                    .map(|(specifier, ordinal)| (specifier, routes[&ordinal].clone()))
                    .collect(),
                modules,
                staged: HashMap::new(),
            },
            initializers,
        )
    }

    fn resolve(&self, name: &str) -> Option<String> {
        self.specifiers.get(name).cloned()
    }

    fn module(&self, route: &str) -> Option<ModuleRegistration> {
        self.modules.get(route).cloned()
    }

    fn host_route(&self, name: &str) -> Option<String> {
        let route = self.specifiers.get(name)?;

        match self.modules.get(route) {
            Some(ModuleRegistration::Host(_)) => Some(route.clone()),
            Some(ModuleRegistration::Native(_)) | None => None,
        }
    }

    fn host_module(&self, route: &str) -> Option<Arc<dyn HostModule>> {
        match self.modules.get(route) {
            Some(ModuleRegistration::Host(module)) => Some(module.clone()),
            Some(ModuleRegistration::Native(_)) | None => None,
        }
    }

    fn stage(&mut self, route: String, namespace: Namespace) {
        match self.staged.entry(route) {
            Entry::Occupied(mut entry) => {
                *entry.get_mut() = namespace;
            }
            Entry::Vacant(entry) => {
                entry.insert(namespace);
            }
        }
    }

    fn take_staged(&mut self, route: &str) -> Option<Namespace> {
        self.staged.remove(route)
    }
}

#[derive(Default)]
struct RegistryState {
    next_guest: u64,
    contexts: HashMap<ContextKey, GuestId>,
    guests: HashMap<GuestId, GuestRegistry>,
}

impl RegistryState {
    fn next_guest(&mut self) -> Result<GuestId, Error> {
        let guest = GuestId(self.next_guest);
        self.next_guest = self
            .next_guest
            .checked_add(1)
            .ok_or_else(|| Error::unexpected("guest identifier space exhausted"))?;

        Ok(guest)
    }

    fn guest(&self, context: ContextKey) -> Option<&GuestRegistry> {
        self.guests
            .get(self.contexts.get(&context)?)
    }

    fn guest_mut(&mut self, context: ContextKey) -> Option<&mut GuestRegistry> {
        self.guests
            .get_mut(self.contexts.get(&context)?)
    }

    fn register(
        &mut self,
        context: ContextKey,
        bindings: &[LibraryBinding],
    ) -> Result<GuestRegistration, Error> {
        let guest = self.next_guest()?;
        let (registry, initializers) = GuestRegistry::new(context, guest, bindings);

        self.contexts
            .insert(registry.context, guest);
        self.guests.insert(guest, registry);

        Ok(GuestRegistration { id: guest, initializers })
    }

    fn resolve(&self, context: ContextKey, name: &str) -> Option<String> {
        self.guest(context)?.resolve(name)
    }

    fn module(&self, context: ContextKey, route: &str) -> Option<ModuleRegistration> {
        self.guest(context)?.module(route)
    }

    fn host_route(&self, context: ContextKey, name: &str) -> Option<String> {
        self.guest(context)?.host_route(name)
    }

    fn host_module(&self, context: ContextKey, route: &str) -> Option<Arc<dyn HostModule>> {
        self.guest(context)?.host_module(route)
    }

    fn stage(&mut self, context: ContextKey, route: String, namespace: Namespace) {
        if let Some(registry) = self.guest_mut(context) {
            registry.stage(route, namespace);
        }
    }

    fn take_staged(&mut self, context: ContextKey, route: &str) -> Option<Namespace> {
        self.guest_mut(context)?
            .take_staged(route)
    }

    fn unregister(&mut self, guest: GuestId) {
        let Some(registry) = self.guests.remove(&guest) else {
            return;
        };

        if self.contexts.get(&registry.context) == Some(&guest) {
            self.contexts.remove(&registry.context);
        }
    }
}

pub(crate) struct ModuleRegistry {
    bindings: Vec<LibraryBinding>,
    state: RefCell<RegistryState>,
}

impl ModuleRegistry {
    pub(crate) fn new(bindings: Vec<LibraryBinding>) -> Self {
        Self {
            bindings,
            state: RefCell::new(RegistryState::default()),
        }
    }

    fn resolve(&self, ctx: &Ctx<'_>, name: &str) -> Option<String> {
        self.state
            .borrow()
            .resolve(ContextKey::new(ctx), name)
    }

    fn module(&self, ctx: &Ctx<'_>, route: &str) -> Option<ModuleRegistration> {
        self.state
            .borrow()
            .module(ContextKey::new(ctx), route)
    }

    pub(crate) fn register_guest(
        &self,
        ctx: &Ctx<'_>,
        bindings: Vec<LibraryBinding>,
    ) -> Result<GuestRegistration, Error> {
        self.state.borrow_mut().register(
            ContextKey::new(ctx),
            &self
                .bindings
                .iter()
                .cloned()
                .chain(bindings)
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn host_route(&self, ctx: &Ctx<'_>, name: &str) -> Result<String, Error> {
        self.state
            .borrow()
            .host_route(ContextKey::new(ctx), name)
            .ok_or_else(|| Error::unexpected(format!("host module `{name}` is not registered")))
    }

    pub(crate) fn host_module(&self, ctx: &Ctx<'_>, route: &str) -> Option<Arc<dyn HostModule>> {
        self.state
            .borrow()
            .host_module(ContextKey::new(ctx), route)
    }

    pub(crate) fn stage(&self, ctx: &Ctx<'_>, route: String, namespace: Namespace) {
        self.state
            .borrow_mut()
            .stage(ContextKey::new(ctx), route, namespace);
    }

    pub(crate) fn take_staged(&self, ctx: &Ctx<'_>, route: &str) -> Option<Namespace> {
        self.state
            .borrow_mut()
            .take_staged(ContextKey::new(ctx), route)
    }

    pub(crate) fn unregister_guest(&self, guest: GuestId) {
        self.state
            .borrow_mut()
            .unregister(guest);
    }
}

pub(crate) struct RegistryHandle(Rc<ModuleRegistry>);

impl RegistryHandle {
    pub(crate) fn new(registry: Rc<ModuleRegistry>) -> Self {
        Self(registry)
    }

    pub(crate) fn registry(&self) -> Rc<ModuleRegistry> {
        self.0.clone()
    }
}

unsafe impl<'js> JsLifetime<'js> for RegistryHandle {
    type Changed<'to> = RegistryHandle;
}

pub(crate) struct ModuleResolver {
    registry: Rc<ModuleRegistry>,
}

impl ModuleResolver {
    pub(crate) fn new(registry: Rc<ModuleRegistry>) -> Self {
        Self { registry }
    }
}

impl Resolver for ModuleResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<String> {
        self.registry
            .resolve(ctx, name)
            .ok_or_else(|| JsError::new_resolving(base, name))
    }
}

pub(crate) struct ModuleLoader {
    registry: Rc<ModuleRegistry>,
}

impl ModuleLoader {
    pub(crate) fn new(registry: Rc<ModuleRegistry>) -> Self {
        Self { registry }
    }
}

impl Loader for ModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<JsModule<'js>> {
        match self.registry.module(ctx, name) {
            Some(ModuleRegistration::Host(_)) => {
                JsModule::declare_def::<HostModuleAdapter, _>(ctx.clone(), name.to_owned())
            }
            Some(ModuleRegistration::Native(module)) => {
                module.declare(ctx.clone(), name.as_bytes().to_vec())
            }
            None => Err(JsError::new_loading(name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use rquickjs::{
        Context as JsContext, Runtime as JsRuntime,
        loader::{Loader, Resolver},
        module::ModuleDef,
    };

    use super::{
        ContextKey, LibraryBinding, ModuleLoader, ModuleRegistration, ModuleRegistry,
        ModuleResolver, RegistryState,
    };
    use crate::{
        host::{Exports, HostModule, Namespace},
        native::{NativeInitializer, NativeModule},
    };

    struct TestHost {
        name: &'static str,
    }

    impl TestHost {
        fn shared(name: &'static str) -> Arc<dyn HostModule> {
            Arc::new(Self { name })
        }

        fn binding(name: &'static str) -> LibraryBinding {
            LibraryBinding::Host(Self::shared(name))
        }
    }

    impl HostModule for TestHost {
        fn name(&self) -> &str {
            self.name
        }

        fn build(&self, _exports: &mut Exports) {}
    }

    struct FirstNative;

    impl ModuleDef for FirstNative {}

    struct SecondNative;

    impl ModuleDef for SecondNative {}

    #[test]
    fn assigns_distinct_guest_ids() {
        let mut state = RegistryState::default();

        let first = state
            .register(ContextKey(1), &[])
            .unwrap()
            .id();
        let second = state
            .register(ContextKey(2), &[])
            .unwrap()
            .id();

        assert_ne!(first, second);
    }

    #[test]
    fn remaps_reused_context_without_merging_guests() {
        let mut state = RegistryState::default();
        let context = ContextKey(1);
        let first = state
            .register(context, &[TestHost::binding("first")])
            .unwrap()
            .id();
        state
            .register(context, &[TestHost::binding("second")])
            .unwrap();

        assert!(
            state
                .resolve(context, "first")
                .is_none()
        );
        assert!(
            state
                .resolve(context, "second")
                .is_some()
        );

        state.unregister(first);

        assert!(
            state
                .resolve(context, "second")
                .is_some()
        );
    }

    #[test]
    fn cleanup_is_scoped_and_idempotent() {
        let mut state = RegistryState::default();
        let first_context = ContextKey(1);
        let second_context = ContextKey(2);
        let first_module = TestHost::shared("first");
        let first = state
            .register(first_context, &[LibraryBinding::Host(first_module.clone())])
            .unwrap()
            .id();
        state
            .register(second_context, &[TestHost::binding("second")])
            .unwrap();

        state.stage(first_context, "staged".to_owned(), Namespace::new());
        state.unregister(first);

        assert_eq!(Arc::strong_count(&first_module), 1);
        assert!(
            state
                .take_staged(first_context, "staged")
                .is_none()
        );
        assert!(
            state
                .resolve(second_context, "second")
                .is_some()
        );

        state.unregister(first);

        assert!(
            state
                .resolve(second_context, "second")
                .is_some()
        );
    }

    #[test]
    fn staged_namespaces_are_isolated_by_guest() {
        let mut state = RegistryState::default();
        let first = ContextKey(1);
        let second = ContextKey(2);

        state.register(first, &[]).unwrap();
        state.register(second, &[]).unwrap();
        state.stage(first, "module".to_owned(), Namespace::new());

        assert!(
            state
                .take_staged(second, "module")
                .is_none()
        );
        assert!(
            state
                .take_staged(first, "module")
                .is_some()
        );
    }

    #[test]
    fn cleanup_preserves_global_bindings() {
        let mut state = RegistryState::default();
        let removed_context = ContextKey(1);
        let remaining_context = ContextKey(2);
        let scoped = TestHost::shared("scoped");
        let removed = state
            .register(
                removed_context,
                &[
                    TestHost::binding("global"),
                    LibraryBinding::Host(scoped.clone()),
                ],
            )
            .unwrap()
            .id();

        state
            .register(remaining_context, &[TestHost::binding("global")])
            .unwrap();
        state.stage(removed_context, "staged".to_owned(), Namespace::new());
        state.unregister(removed);

        assert_eq!(Arc::strong_count(&scoped), 1);
        assert!(
            state
                .resolve(removed_context, "scoped")
                .is_none()
        );
        assert!(
            state
                .take_staged(removed_context, "staged")
                .is_none()
        );
        assert!(
            state
                .resolve(remaining_context, "global")
                .is_some()
        );
    }

    #[test]
    fn global_host_is_inherited_by_multiple_guests() {
        let module = TestHost::shared("shared");
        let bindings = [LibraryBinding::Host(module.clone())];
        let mut state = RegistryState::default();
        let first = ContextKey(1);
        let second = ContextKey(2);

        state
            .register(first, &bindings)
            .unwrap();
        state
            .register(second, &bindings)
            .unwrap();

        assert!(Arc::ptr_eq(
            &state
                .host_module(first, &state.resolve(first, "shared").unwrap(),)
                .unwrap(),
            &module,
        ));
        assert!(Arc::ptr_eq(
            &state
                .host_module(second, &state.resolve(second, "shared").unwrap(),)
                .unwrap(),
            &module,
        ));
    }

    #[test]
    fn local_host_replaces_global_host() {
        let local = TestHost::shared("shared");
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[
                    LibraryBinding::Host(TestHost::shared("shared")),
                    LibraryBinding::Host(local.clone()),
                ],
            )
            .unwrap();

        assert!(Arc::ptr_eq(
            &state
                .host_module(
                    context,
                    &state
                        .resolve(context, "shared")
                        .unwrap(),
                )
                .unwrap(),
            &local,
        ));
    }

    #[test]
    fn native_replaces_earlier_host() {
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[
                    TestHost::binding("shared"),
                    LibraryBinding::Native(NativeModule::new("shared", FirstNative)),
                ],
            )
            .unwrap();

        assert!(matches!(
            state.module(
                context,
                &state
                    .resolve(context, "shared")
                    .unwrap(),
            ),
            Some(ModuleRegistration::Native(_)),
        ));
    }

    #[test]
    fn host_replaces_earlier_native() {
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[
                    LibraryBinding::Native(NativeModule::new("shared", FirstNative)),
                    TestHost::binding("shared"),
                ],
            )
            .unwrap();

        assert!(matches!(
            state.module(
                context,
                &state
                    .resolve(context, "shared")
                    .unwrap(),
            ),
            Some(ModuleRegistration::Host(_)),
        ));
    }

    #[test]
    fn later_library_entry_wins() {
        let second = TestHost::shared("shared");
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[
                    LibraryBinding::Host(TestHost::shared("shared")),
                    LibraryBinding::Host(second.clone()),
                ],
            )
            .unwrap();

        assert!(Arc::ptr_eq(
            &state
                .host_module(
                    context,
                    &state
                        .resolve(context, "shared")
                        .unwrap(),
                )
                .unwrap(),
            &second,
        ));
    }

    #[test]
    fn aliases_share_one_private_route() {
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[LibraryBinding::Native(
                    NativeModule::new("module", FirstNative)
                        .alias("module:alias")
                        .alias("node:module"),
                )],
            )
            .unwrap();

        assert_eq!(state.resolve(context, "module"), state.resolve(context, "module:alias"),);
        assert_eq!(state.resolve(context, "module"), state.resolve(context, "node:module"),);
    }

    #[test]
    fn replacing_alias_preserves_other_specifiers() {
        let mut state = RegistryState::default();
        let context = ContextKey(1);

        state
            .register(
                context,
                &[
                    LibraryBinding::Native(
                        NativeModule::new("module", FirstNative)
                            .alias("module:alias")
                            .alias("node:module"),
                    ),
                    LibraryBinding::Native(NativeModule::new("node:module", SecondNative)),
                ],
            )
            .unwrap();

        assert_eq!(state.resolve(context, "module"), state.resolve(context, "module:alias"),);
        assert_ne!(state.resolve(context, "module"), state.resolve(context, "node:module"),);
    }

    #[test]
    fn overwritten_native_omits_associated_initialization() {
        let mut state = RegistryState::default();

        assert!(
            state
                .register(
                    ContextKey(1),
                    &[
                        LibraryBinding::Native(
                            NativeModule::new("shared", FirstNative)
                                .initialize(NativeInitializer::new("native:init", |_ctx| Ok(()))),
                        ),
                        TestHost::binding("shared"),
                    ],
                )
                .unwrap()
                .into_initializers()
                .is_empty()
        );
    }

    #[test]
    fn standalone_initializer_survives_module_override() {
        let mut state = RegistryState::default();

        assert_eq!(
            state
                .register(
                    ContextKey(1),
                    &[
                        LibraryBinding::Initializer(NativeInitializer::new(
                            "dependency:init",
                            |_ctx| Ok(()),
                        )),
                        LibraryBinding::Native(
                            NativeModule::new("shared", FirstNative)
                                .initialize(NativeInitializer::new("module:init", |_ctx| Ok(())),),
                        ),
                        TestHost::binding("shared"),
                    ],
                )
                .unwrap()
                .into_initializers()
                .iter()
                .map(|initializer| initializer.name())
                .collect::<Vec<_>>(),
            vec!["dependency:init"],
        );
    }

    #[test]
    fn duplicate_initializers_keep_last_callback_in_final_order() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut state = RegistryState::default();
        let initializers = state
            .register(
                ContextKey(1),
                &[
                    LibraryBinding::Initializer(NativeInitializer::new("shared", {
                        let calls = calls.clone();

                        move |_ctx| {
                            calls.borrow_mut().push("first");

                            Ok(())
                        }
                    })),
                    LibraryBinding::Initializer(NativeInitializer::new("other", {
                        let calls = calls.clone();

                        move |_ctx| {
                            calls.borrow_mut().push("other");

                            Ok(())
                        }
                    })),
                    LibraryBinding::Initializer(NativeInitializer::new("shared", {
                        let calls = calls.clone();

                        move |_ctx| {
                            calls.borrow_mut().push("last");

                            Ok(())
                        }
                    })),
                ],
            )
            .unwrap()
            .into_initializers();
        let runtime = JsRuntime::new().unwrap();
        let context = JsContext::full(&runtime).unwrap();

        assert_eq!(
            initializers
                .iter()
                .map(|initializer| initializer.name())
                .collect::<Vec<_>>(),
            vec!["other", "shared"],
        );

        context.with(|ctx| {
            for initializer in initializers {
                initializer.initialize(&ctx).unwrap();
            }
        });

        assert_eq!(*calls.borrow(), vec!["other", "last"]);
    }

    #[test]
    fn host_lookup_rejects_native_winner() {
        let registry = ModuleRegistry::new(vec![LibraryBinding::Native(NativeModule::new(
            "shared",
            FirstNative,
        ))]);
        let runtime = JsRuntime::new().unwrap();
        let context = JsContext::full(&runtime).unwrap();

        context.with(|ctx| {
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            assert_eq!(
                registry
                    .host_route(&ctx, "shared")
                    .unwrap_err()
                    .to_string(),
                "unexpected error: host module `shared` is not registered",
            );
        });
    }

    #[test]
    fn native_definition_loads_in_distinct_contexts() {
        let registry = Rc::new(ModuleRegistry::new(vec![LibraryBinding::Native(
            NativeModule::new("native", FirstNative),
        )]));
        let runtime = JsRuntime::new().unwrap();
        let first = JsContext::full(&runtime).unwrap();
        let second = JsContext::full(&runtime).unwrap();

        first.with(|ctx| {
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            ModuleLoader::new(registry.clone())
                .load(
                    &ctx,
                    &ModuleResolver::new(registry.clone())
                        .resolve(&ctx, "entry", "native", None)
                        .unwrap(),
                    None,
                )
                .unwrap();
        });
        second.with(|ctx| {
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            ModuleLoader::new(registry.clone())
                .load(
                    &ctx,
                    &ModuleResolver::new(registry.clone())
                        .resolve(&ctx, "entry", "native", None)
                        .unwrap(),
                    None,
                )
                .unwrap();
        });
    }

    #[test]
    fn loader_rejects_another_guests_private_route() {
        let registry = Rc::new(ModuleRegistry::new(vec![LibraryBinding::Native(
            NativeModule::new("native", FirstNative),
        )]));
        let runtime = JsRuntime::new().unwrap();
        let first = JsContext::full(&runtime).unwrap();
        let second = JsContext::full(&runtime).unwrap();
        let first_route = first.with(|ctx| {
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            ModuleResolver::new(registry.clone())
                .resolve(&ctx, "entry", "native", None)
                .unwrap()
        });

        second.with(|ctx| {
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            assert!(
                ModuleLoader::new(registry.clone())
                    .load(&ctx, &first_route, None)
                    .err()
                    .unwrap()
                    .is_loading()
            );
        });
    }
}
