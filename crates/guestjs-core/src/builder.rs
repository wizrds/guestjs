use std::time::Duration;

use crate::{
    backend::{
        interrupt::BackendInterrupt,
        modules::BackendModules,
    },
    bundle::Bundle,
    catalog::Catalog,
    errors::Error,
    guest::Guest,
    host::{
        library::HostLibrary,
        native::NativeLibrary,
    },
    imports::{
        bindings::GuestBindings,
        Imports,
    },
    policy::{
        CancelSignal,
        ExecutionPolicy,
    },
    runtime::Runtime,
};

pub struct RuntimeBuilder<B: BackendModules> {
    config: B::Config,
    policy: ExecutionPolicy,
    catalog: Result<Catalog<B>, Error>,
}

impl<B: BackendModules> RuntimeBuilder<B> {
    pub fn new() -> Self {
        Self {
            config: B::Config::default(),
            policy: ExecutionPolicy::default(),
            catalog: Ok(Catalog::new()),
        }
    }

    pub fn config(mut self, config: B::Config) -> Self {
        self.config = config;

        self
    }

    pub fn bind<L>(mut self, library: L) -> Self
    where
        L: Into<HostLibrary<B>>,
    {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.host(library));

        self
    }

    pub fn bind_native<L>(mut self, library: L) -> Self
    where
        L: Into<NativeLibrary<B>>,
    {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.native(library));

        self
    }

    pub fn bundle(mut self, bundle: &Bundle) -> Self {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.bundle(bundle));

        self
    }

    pub fn deny<N>(mut self, name: N) -> Self
    where
        N: Into<String>,
    {
        self.catalog = self
            .catalog
            .map(|catalog| catalog.deny(name));

        self
    }

    pub fn build(self) -> Result<Runtime<B>, Error> {
        self.catalog.and_then(|catalog| {
            let runtime = Runtime::new(self.config)?;

            Imports::install(&runtime, catalog)?;

            runtime.extensions().insert(self.policy);

            Ok(runtime)
        })
    }
}

impl<B> RuntimeBuilder<B>
where
    B: BackendModules + BackendInterrupt,
{
    pub fn policy(mut self, policy: ExecutionPolicy) -> Self {
        self.policy = policy;

        self
    }

    pub fn timeout<T>(mut self, timeout: T) -> Self
    where
        T: Into<Duration>,
    {
        self.policy = self.policy.timeout(timeout);

        self
    }

    pub fn cancellation<S>(mut self, signal: S) -> Self
    where
        S: CancelSignal,
    {
        self.policy = self.policy.cancellation(signal);

        self
    }
}

impl<B: BackendModules> Default for RuntimeBuilder<B> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait RuntimeBuilderExt<B>
where
    B: BackendModules,
{
    fn builder() -> RuntimeBuilder<B>;
}

impl<B> RuntimeBuilderExt<B> for Runtime<B>
where
    B: BackendModules,
{
    fn builder() -> RuntimeBuilder<B> {
        RuntimeBuilder::new()
    }
}

pub struct GuestBuilder<'r, B: BackendModules> {
    runtime: &'r Runtime<B>,
    policy: ExecutionPolicy,
    catalog: Result<Catalog<B>, Error>,
}

impl<'r, B: BackendModules> GuestBuilder<'r, B> {
    pub fn new(runtime: &'r Runtime<B>) -> Self {
        Self {
            runtime,
            policy: runtime
                .extensions()
                .get::<ExecutionPolicy>()
                .as_deref()
                .cloned()
                .unwrap_or_default(),
            catalog: Ok(Catalog::new()),
        }
    }

    pub fn bind<L>(mut self, library: L) -> Self
    where
        L: Into<HostLibrary<B>>,
    {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.host(library));

        self
    }

    pub fn bind_native<L>(mut self, library: L) -> Self
    where
        L: Into<NativeLibrary<B>>,
    {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.native(library));

        self
    }

    pub fn bundle(mut self, bundle: &Bundle) -> Self {
        self.catalog = self
            .catalog
            .and_then(|catalog| catalog.bundle(bundle));

        self
    }

    pub fn deny<N>(mut self, name: N) -> Self
    where
        N: Into<String>,
    {
        self.catalog = self
            .catalog
            .map(|catalog| catalog.deny(name));

        self
    }
}

impl<'r, B> GuestBuilder<'r, B>
where
    B: BackendModules + BackendInterrupt,
{
    pub fn policy(mut self, policy: ExecutionPolicy) -> Self {
        self.policy = policy;

        self
    }

    pub fn timeout<T>(mut self, timeout: T) -> Self
    where
        T: Into<Duration>,
    {
        self.policy = self.policy.timeout(timeout);

        self
    }

    pub fn build(self) -> Result<Guest<B>, Error> {
        self.catalog.and_then(|catalog| {
            let guest = Guest::with_policy(self.runtime, self.policy)?;
            let bindings = guest
                .extensions()
                .insert(GuestBindings::new(
                    Catalog::of(self.runtime),
                    catalog,
                ));

            guest.enter(|enter| {
                bindings
                    .runtime_catalog()
                    .host_initializers()
                    .iter()
                    .chain(bindings.guest_catalog().host_initializers())
                    .try_for_each(|initializer| initializer.run(enter))?;

                bindings
                    .runtime_catalog()
                    .native_initializers()
                    .iter()
                    .chain(bindings.guest_catalog().native_initializers())
                    .try_for_each(|initializer| initializer.run(enter))
            })?;

            Ok(guest)
        })
    }
}

pub trait GuestBuilderExt<B>
where
    B: BackendModules,
{
    fn guest(&self) -> GuestBuilder<'_, B>;
}

impl<B> GuestBuilderExt<B> for Runtime<B>
where
    B: BackendModules,
{
    fn guest(&self) -> GuestBuilder<'_, B> {
        GuestBuilder::new(self)
    }
}
