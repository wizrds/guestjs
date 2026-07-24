use std::{
    cell::RefCell,
    collections::HashMap,
    sync::Arc,
};

use rquickjs::{
    Ctx,
    JsLifetime,
};

use crate::{
    errors::Error,
    host::{
        module::HostModule,
        namespace::Namespace,
    },
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GuestId(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ContextKey(usize);

impl ContextKey {
    fn new(ctx: &Ctx<'_>) -> Self {
        Self(ctx.as_raw().as_ptr() as usize)
    }
}

struct GuestRegistry {
    context: ContextKey,
    scoped: HashMap<String, Arc<dyn HostModule>>,
    staged: HashMap<String, Namespace>,
}

impl GuestRegistry {
    fn new(context: ContextKey) -> Self {
        Self {
            context,
            scoped: HashMap::new(),
            staged: HashMap::new(),
        }
    }
}

struct RegistryState {
    next_guest: u64,
    contexts: HashMap<ContextKey, GuestId>,
    global: HashMap<String, Arc<dyn HostModule>>,
    guests: HashMap<GuestId, GuestRegistry>,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            next_guest: 0,
            contexts: HashMap::new(),
            global: HashMap::new(),
            guests: HashMap::new(),
        }
    }
}

impl RegistryState {
    fn register_guest(&mut self, context: ContextKey) -> Result<GuestId, Error> {
        let next_guest = self
            .next_guest
            .checked_add(1)
            .ok_or_else(|| Error::unexpected("guest identifier space exhausted"))?;
        let guest = GuestId(self.next_guest);

        self.next_guest = next_guest;
        self.contexts.insert(context, guest);
        self.guests.insert(guest, GuestRegistry::new(context));

        Ok(guest)
    }

    fn guest_id(&self, context: ContextKey) -> Option<GuestId> {
        self.contexts.get(&context).copied()
    }

    fn register_global(
        &mut self,
        module: Arc<dyn HostModule>,
    ) -> Option<Arc<dyn HostModule>> {
        self.global.insert(module.name().to_owned(), module)
    }

    fn register_scoped(
        &mut self,
        guest: GuestId,
        module: Arc<dyn HostModule>,
    ) -> Option<Arc<dyn HostModule>> {
        match self.guests.get_mut(&guest) {
            Some(registry) => registry
                .scoped
                .insert(module.name().to_owned(), module),
            None => Some(module),
        }
    }

    fn is_known(&self, guest: GuestId, name: &str) -> bool {
        let Some(registry) = self.guests.get(&guest) else {
            return false;
        };

        registry.scoped.contains_key(name) || self.global.contains_key(name)
    }

    fn lookup(&self, guest: GuestId, name: &str) -> Option<Arc<dyn HostModule>> {
        let registry = self.guests.get(&guest)?;

        registry
            .scoped
            .get(name)
            .or_else(|| self.global.get(name))
            .cloned()
    }

    fn stage(
        &mut self,
        guest: GuestId,
        name: String,
        namespace: Namespace,
    ) -> Option<Namespace> {
        match self.guests.get_mut(&guest) {
            Some(registry) => registry.staged.insert(name, namespace),
            None => Some(namespace),
        }
    }

    fn take_staged(&mut self, guest: GuestId, name: &str) -> Option<Namespace> {
        self.guests
            .get_mut(&guest)
            .and_then(|registry| registry.staged.remove(name))
    }

    fn unregister_guest(&mut self, guest: GuestId) -> Option<GuestRegistry> {
        let registry = self.guests.remove(&guest)?;

        if self.contexts.get(&registry.context) == Some(&guest) {
            self.contexts.remove(&registry.context);
        }

        Some(registry)
    }
}

/// A runtime-wide host-module registry.
#[derive(Default)]
pub(crate) struct HostRegistry {
    state: RefCell<RegistryState>,
}

impl HostRegistry {
    fn register_context(&self, context: ContextKey) -> Result<GuestId, Error> {
        self.state.borrow_mut().register_guest(context)
    }

    fn resolve_context(&self, context: ContextKey) -> Option<GuestId> {
        self.state.borrow().guest_id(context)
    }

    pub(crate) fn register_guest(&self, ctx: &Ctx<'_>) -> Result<GuestId, Error> {
        self.register_context(ContextKey::new(ctx))
    }

    pub(crate) fn guest_id(&self, ctx: &Ctx<'_>) -> Option<GuestId> {
        self.resolve_context(ContextKey::new(ctx))
    }

    pub(crate) fn register_global(&self, module: Arc<dyn HostModule>) {
        let _released = self.state.borrow_mut().register_global(module);
    }

    pub(crate) fn register_scoped(&self, guest: GuestId, module: Arc<dyn HostModule>) {
        let _released = self.state.borrow_mut().register_scoped(guest, module);
    }

    pub(crate) fn is_known(&self, guest: GuestId, name: &str) -> bool {
        self.state.borrow().is_known(guest, name)
    }

    pub(crate) fn lookup(
        &self,
        guest: GuestId,
        name: &str,
    ) -> Option<Arc<dyn HostModule>> {
        self.state.borrow().lookup(guest, name)
    }

    pub(crate) fn stage(&self, guest: GuestId, name: String, namespace: Namespace) {
        let _released = self.state.borrow_mut().stage(guest, name, namespace);
    }

    pub(crate) fn take_staged(&self, guest: GuestId, name: &str) -> Option<Namespace> {
        self.state.borrow_mut().take_staged(guest, name)
    }

    pub(crate) fn unregister_guest(&self, guest: GuestId) {
        let _removed = self.state.borrow_mut().unregister_guest(guest);
    }
}

/// A runtime userdata handle for the host registry.
pub(crate) struct RegistryHandle(pub(crate) Arc<HostRegistry>);

unsafe impl<'js> JsLifetime<'js> for RegistryHandle {
    type Changed<'to> = RegistryHandle;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        ContextKey,
        HostRegistry,
    };
    use crate::host::{
        module::{
            Exports,
            HostModule,
        },
        namespace::Namespace,
    };

    struct TestModule {
        name: &'static str,
    }

    impl TestModule {
        fn shared(name: &'static str) -> Arc<dyn HostModule> {
            Arc::new(Self { name })
        }
    }

    impl HostModule for TestModule {
        fn name(&self) -> &str {
            self.name
        }

        fn build(&self, _exports: &mut Exports) {}
    }

    #[test]
    fn assigns_distinct_guest_ids() {
        let registry = HostRegistry::default();

        let first = registry.register_context(ContextKey(1)).unwrap();
        let second = registry.register_context(ContextKey(2)).unwrap();

        assert_ne!(first, second);
        assert_eq!(registry.resolve_context(ContextKey(1)), Some(first));
        assert_eq!(registry.resolve_context(ContextKey(2)), Some(second));
    }

    #[test]
    fn remaps_reused_context_without_merging_guests() {
        let registry = HostRegistry::default();
        let context = ContextKey(1);

        let first = registry.register_context(context).unwrap();
        registry.register_scoped(first, TestModule::shared("first"));

        let second = registry.register_context(context).unwrap();
        registry.register_scoped(second, TestModule::shared("second"));

        assert_eq!(registry.resolve_context(context), Some(second));
        assert!(registry.lookup(first, "first").is_some());
        assert!(registry.lookup(first, "second").is_none());
        assert!(registry.lookup(second, "first").is_none());
        assert!(registry.lookup(second, "second").is_some());

        registry.unregister_guest(first);

        assert_eq!(registry.resolve_context(context), Some(second));
    }

    #[test]
    fn cleanup_is_scoped_and_idempotent() {
        let registry = HostRegistry::default();
        let first = registry.register_context(ContextKey(1)).unwrap();
        let second = registry.register_context(ContextKey(2)).unwrap();
        let first_module = TestModule::shared("first");

        registry.register_scoped(first, first_module.clone());
        registry.register_scoped(second, TestModule::shared("second"));
        registry.stage(first, "staged".to_owned(), Namespace::new());

        registry.unregister_guest(first);

        assert_eq!(Arc::strong_count(&first_module), 1);
        assert_eq!(registry.resolve_context(ContextKey(1)), None);
        assert_eq!(registry.resolve_context(ContextKey(2)), Some(second));
        assert!(registry.take_staged(first, "staged").is_none());
        assert!(registry.lookup(second, "second").is_some());

        registry.unregister_guest(first);

        assert_eq!(registry.resolve_context(ContextKey(2)), Some(second));
        assert!(registry.lookup(second, "second").is_some());
    }

    #[test]
    fn scoped_modules_precede_global_modules() {
        let registry = HostRegistry::default();
        let guest = registry.register_context(ContextKey(1)).unwrap();
        let scoped = TestModule::shared("shared");

        registry.register_global(TestModule::shared("shared"));
        registry.register_scoped(guest, scoped.clone());

        assert!(Arc::ptr_eq(
            &registry.lookup(guest, "shared").unwrap(),
            &scoped,
        ));
    }

    #[test]
    fn staged_namespaces_are_isolated_by_guest() {
        let registry = HostRegistry::default();
        let first = registry.register_context(ContextKey(1)).unwrap();
        let second = registry.register_context(ContextKey(2)).unwrap();

        registry.stage(first, "module".to_owned(), Namespace::new());

        assert!(registry.take_staged(second, "module").is_none());
        assert!(registry.take_staged(first, "module").is_some());
    }

    #[test]
    fn cleanup_removes_guest_state_and_preserves_globals() {
        let registry = HostRegistry::default();
        let context = ContextKey(1);
        let guest = registry.register_context(context).unwrap();
        let remaining = registry.register_context(ContextKey(2)).unwrap();
        let scoped = TestModule::shared("scoped");

        registry.register_global(TestModule::shared("global"));
        registry.register_scoped(guest, scoped.clone());
        registry.stage(guest, "staged".to_owned(), Namespace::new());

        registry.unregister_guest(guest);

        assert_eq!(registry.resolve_context(context), None);
        assert_eq!(Arc::strong_count(&scoped), 1);
        assert!(registry.lookup(guest, "scoped").is_none());
        assert!(registry.take_staged(guest, "staged").is_none());
        assert!(registry.lookup(remaining, "global").is_some());
    }
}
