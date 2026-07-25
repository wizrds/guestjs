use std::{
    ops::Deref,
    rc::{Rc, Weak},
    sync::Arc,
    time::Duration,
};

use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Module as JsModule, Persistent, Value,
};

use crate::{
    errors::Error,
    execution::{CancelSignal, ExecutionPolicy},
    handle::{BoundModule, Module, Object},
    host::{HostLibrary, HostModuleAdapter},
    marshal::FromGuest,
    native::NativeLibrary,
    registry::{
        GuestId, LibraryBinding, ModuleLoader, ModuleRegistry, ModuleResolver, RegistryHandle,
    },
    transpiler::Transpiler,
};

#[cfg(feature = "typescript")]
use crate::transpiler::OxcTranspiler;

/// The top-level JavaScript engine.
#[derive(Clone)]
pub struct Runtime {
    inner: AsyncRuntime,
    registry: Rc<ModuleRegistry>,
    policy: ExecutionPolicy,
    transpiler: Option<Arc<dyn Transpiler>>,
}

impl Runtime {
    /// Creates a builder for configuring a runtime.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Creates a builder for an isolated guest.
    pub fn guest(&self) -> GuestBuilder<'_> {
        GuestBuilder { runtime: self, bindings: Vec::new() }
    }

    /// Runs a garbage-collection cycle.
    pub async fn run_gc(&self) {
        self.inner.run_gc().await;
    }
}

/// Configures and constructs a [`Runtime`](crate::runtime::Runtime).
#[derive(Default)]
pub struct RuntimeBuilder {
    bindings: Vec<LibraryBinding>,
    memory_limit: Option<usize>,
    max_stack_size: Option<usize>,
    gc_threshold: Option<usize>,
    timeout: Option<Duration>,
    cancellation: Option<Arc<dyn CancelSignal>>,
    gc_after: Option<u32>,
    interrupt_handler: Option<Box<dyn FnMut() -> bool + Send + 'static>>,
    transpiler: Option<Arc<dyn Transpiler>>,
}

impl RuntimeBuilder {
    /// Adds a [`HostLibrary`](crate::host::HostLibrary).
    pub fn bind(mut self, library: impl Into<HostLibrary>) -> Self {
        self.bindings
            .extend(LibraryBinding::from_host(library.into()));

        self
    }

    /// Adds a [`NativeLibrary`](crate::native::NativeLibrary).
    pub fn bind_native(mut self, library: impl Into<NativeLibrary>) -> Self {
        self.bindings
            .extend(LibraryBinding::from_native(library.into()));

        self
    }

    /// Limits the number of bytes the runtime may allocate.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// Limits the engine call-stack size in bytes.
    pub fn max_stack_size(mut self, bytes: usize) -> Self {
        self.max_stack_size = Some(bytes);
        self
    }

    /// Sets the allocation threshold that triggers garbage collection.
    pub fn gc_threshold(mut self, bytes: usize) -> Self {
        self.gc_threshold = Some(bytes);
        self
    }

    /// Sets the time budget for each guest execution.
    pub fn execution_timeout(mut self, budget: Duration) -> Self {
        self.timeout = Some(budget);
        self
    }

    /// Sets the cancellation signal for guest executions.
    pub fn cancellation<S>(mut self, signal: S) -> Self
    where
        S: CancelSignal,
    {
        self.cancellation = Some(Arc::new(signal));
        self
    }

    /// Sets the number of guest executions between garbage collections.
    pub fn gc_after(mut self, executions: u32) -> Self {
        self.gc_after = Some(executions);
        self
    }

    /// Sets the engine interrupt handler.
    pub fn interrupt_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut() -> bool + Send + 'static,
    {
        self.interrupt_handler = Some(Box::new(handler));
        self
    }

    /// Sets the transpiler applied to source before it is loaded.
    pub fn transpiler<T>(mut self, transpiler: T) -> Self
    where
        T: Transpiler + 'static,
    {
        self.transpiler = Some(Arc::new(transpiler));
        self
    }

    /// Builds the configured runtime.
    pub async fn build(self) -> Result<Runtime, Error> {
        let inner = AsyncRuntime::new()
            .map_err(|error| Error::sourced_engine(error.to_string(), Some(error)))?;

        if let Some(bytes) = self.memory_limit {
            inner.set_memory_limit(bytes).await;
        }

        if let Some(bytes) = self.max_stack_size {
            inner.set_max_stack_size(bytes).await;
        }

        if let Some(bytes) = self.gc_threshold {
            inner.set_gc_threshold(bytes).await;
        }

        let policy = ExecutionPolicy::new(self.timeout, self.cancellation, self.gc_after);

        inner
            .set_interrupt_handler(Some({
                let policy = policy.clone();
                let mut user = self.interrupt_handler;

                Box::new(move || {
                    policy.should_abort() || user.as_mut().is_some_and(|handler| handler())
                })
            }))
            .await;

        let registry = Rc::new(ModuleRegistry::new(self.bindings));

        inner
            .set_loader(ModuleResolver::new(registry.clone()), ModuleLoader::new(registry.clone()))
            .await;

        Ok(Runtime {
            inner,
            registry,
            policy,
            transpiler: self.transpiler.or_else(|| {
                if cfg!(feature = "typescript") {
                    Some(Arc::new(OxcTranspiler))
                } else {
                    None
                }
            }),
        })
    }
}

/// Configures and constructs a [`Guest`](crate::runtime::Guest).
pub struct GuestBuilder<'runtime> {
    runtime: &'runtime Runtime,
    bindings: Vec<LibraryBinding>,
}

impl GuestBuilder<'_> {
    /// Adds a [`HostLibrary`](crate::host::HostLibrary).
    pub fn bind(mut self, library: impl Into<HostLibrary>) -> Self {
        self.bindings
            .extend(LibraryBinding::from_host(library.into()));

        self
    }

    /// Adds a [`NativeLibrary`](crate::native::NativeLibrary).
    pub fn bind_native(mut self, library: impl Into<NativeLibrary>) -> Self {
        self.bindings
            .extend(LibraryBinding::from_native(library.into()));

        self
    }

    /// Builds the configured guest.
    pub async fn build(self) -> Result<Guest, Error> {
        let Self { runtime, bindings } = self;

        let inner = AsyncContext::full(&runtime.inner)
            .await
            .map_err(|error| Error::sourced_engine(error.to_string(), Some(error)))?;

        let registration = inner
            .async_with(async move |ctx| {
                if ctx
                    .userdata::<RegistryHandle>()
                    .is_none()
                {
                    ctx.store_userdata(RegistryHandle::new(runtime.registry.clone()))
                        .map_err(|_| {
                            Error::unexpected("failed to store the module registry as userdata")
                        })?;
                }

                runtime
                    .registry
                    .register_guest(&ctx, bindings)
            })
            .await?;

        let context = Rc::new(
            GuestContext {
                inner,
                id: registration.id(),
                registry: Rc::downgrade(&runtime.registry),
                policy: runtime.policy.clone(),
                transpiler: runtime.transpiler.clone(),
            },
        );

        Scope::with(&context, async move |scope| {
            for initializer in registration.into_initializers() {
                initializer.initialize(scope.ctx())?;
            }

            Ok(())
        })
        .await?;

        Ok(Guest { context })
    }
}

/// The owning context for a guest.
pub struct GuestContext {
    inner: AsyncContext,
    id: GuestId,
    registry: Weak<ModuleRegistry>,
    policy: ExecutionPolicy,
    transpiler: Option<Arc<dyn Transpiler>>,
}

impl GuestContext {
    fn transpile(&self, name: &str, source: &str) -> Result<String, Error> {
        match &self.transpiler {
            Some(transpiler) => transpiler.transpile(name, source),
            None => Ok(source.to_owned()),
        }
    }
}

impl Drop for GuestContext {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.unregister_guest(self.id);
        }
    }
}

impl Deref for GuestContext {
    type Target = AsyncContext;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// An isolated guest execution environment.
#[derive(Clone)]
pub struct Guest {
    context: Rc<GuestContext>,
}

impl Guest {
    /// Runs operations within one guest scope.
    pub async fn scope<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'js> AsyncFnOnce(Scope<'js>) -> Result<R, Error>,
        R: 'static,
    {
        Scope::with(&self.context, async move |scope| f(scope).await).await
    }

    /// Evaluates JavaScript and converts its result into a Rust value.
    pub async fn eval<R>(&self, source: impl Into<Vec<u8>>) -> Result<R::Owned, Error>
    where
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(
                &scope,
                scope
                    .ctx()
                    .eval::<Value, _>(source)
                    .catch(scope.ctx())?,
            )
        })
        .await
    }

    /// Returns an owned handle to this guest's global object.
    pub async fn globals(&self) -> Result<Object, Error> {
        Scope::with(&self.context, async move |scope| {
            Ok(Object::new(
                Persistent::save(scope.ctx(), scope.ctx().globals()),
                scope
                    .parent()
                    .ok_or_else(Error::detached_scope)?
                    .clone(),
            ))
        })
        .await
    }

    /// Loads a guest JavaScript module.
    pub async fn guest_module(
        &self,
        name: &str,
        source: impl Into<String>,
    ) -> Result<Module, Error> {
        Scope::with(&self.context, async move |scope| {
            scope
                .guest_module(name, source)
                .await?
                .into_owned()
        })
        .await
    }

    /// Instantiates a registered host binding in this guest and returns a handle to its exports.
    pub async fn host_module(&self, name: &str) -> Result<Module, Error> {
        Scope::with(&self.context, async move |scope| {
            scope
                .host_module(name)
                .await?
                .into_owned()
        })
        .await
    }
}

/// A live guest execution scope.
#[derive(Clone)]
pub struct Scope<'js> {
    ctx: Ctx<'js>,
    parent: Option<Rc<GuestContext>>,
}

impl<'js> Scope<'js> {
    pub(crate) fn new(ctx: Ctx<'js>, parent: Rc<GuestContext>) -> Self {
        Self { ctx, parent: Some(parent) }
    }

    pub(crate) fn detached(ctx: Ctx<'js>) -> Self {
        Self { ctx, parent: None }
    }

    pub(crate) async fn with<F, R>(context: &Rc<GuestContext>, f: F) -> Result<R, Error>
    where
        F: for<'a> AsyncFnOnce(Scope<'a>) -> Result<R, Error>,
        R: 'static,
    {
        context.policy.begin()?;

        let result = context.policy.classify(
            context
                .async_with(async move |ctx| f(Scope::new(ctx, context.clone())).await)
                .await,
        );

        context.policy.disarm();

        if context.policy.should_gc() {
            context.runtime().run_gc().await;
        }

        result
    }

    /// Returns the live engine context.
    pub fn ctx(&self) -> &Ctx<'js> {
        &self.ctx
    }

    /// Returns the owning context when the scope supports re-entry.
    pub fn parent(&self) -> Option<&Rc<GuestContext>> {
        self.parent.as_ref()
    }

    /// Loads a guest module.
    pub async fn guest_module(
        &self,
        name: &str,
        source: impl Into<String>,
    ) -> Result<BoundModule<'js>, Error> {
        let (module, promise) = JsModule::declare(
            self.ctx().clone(),
            name.to_owned(),
            self.parent()
                .ok_or_else(Error::detached_scope)?
                .transpile(name, &source.into())?
                .into_bytes(),
        )
        .catch(self.ctx())?
        .eval()
        .catch(self.ctx())?;

        promise
            .into_future::<()>()
            .await
            .catch(self.ctx())?;

        Ok(BoundModule::new(module.namespace().catch(self.ctx())?, self.clone()))
    }

    /// Loads a registered host module.
    pub async fn host_module(&self, name: &str) -> Result<BoundModule<'js>, Error> {
        self.parent()
            .ok_or_else(Error::detached_scope)?;

        let (module, promise) = JsModule::declare_def::<HostModuleAdapter, _>(
            self.ctx().clone(),
            self.ctx()
                .userdata::<RegistryHandle>()
                .ok_or_else(|| Error::unexpected("module registry is not installed"))?
                .registry()
                .host_route(self.ctx(), name)?,
        )
        .catch(self.ctx())?
        .eval()
        .catch(self.ctx())?;

        promise
            .into_future::<()>()
            .await
            .catch(self.ctx())?;

        Ok(BoundModule::new(module.namespace().catch(self.ctx())?, self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use rquickjs::{
        Ctx, Error as JsError,
        module::{Declarations, Exports as JsExports, ModuleDef},
    };
    #[cfg(feature = "tokio")]
    use tokio_util::sync::CancellationToken;

    use super::{Runtime, Scope};
    use crate::{
        errors::Error,
        execution::Cancellation,
        handle::Function,
        host::{Exports, HostLibrary, HostModule},
        native::{NativeInitializer, NativeLibrary, NativeModule},
    };

    const SCOPED_GUEST_MODULE_SOURCE: &str = r#"
        export const settings = {
            prefix: "hello",
        };

        export function greet(name) {
            return `${settings.prefix} ${name}`;
        }
    "#;

    const IMPORT_SHARED_VALUE_SOURCE: &str = r#"
        import { value } from "shared";

        export default value;
    "#;

    const IMPORT_MULTIPLE_NATIVE_SOURCE: &str = r#"
        import { value as first } from "first";
        import { value as second } from "second";

        export default first + second;
    "#;

    #[cfg(feature = "typescript")]
    const SCOPED_TYPESCRIPT_MODULE_SOURCE: &str = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        export function pick(): Color {
            return Color.Blue;
        }
    "#;

    struct ArithmeticHostModule;

    impl HostModule for ArithmeticHostModule {
        fn name(&self) -> &str {
            "@host/arithmetic"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("multiply", |scope, args| {
                Ok(args.get::<i32>(scope, 0)? * args.get::<i32>(scope, 1)?)
            });
        }
    }

    struct ValueHost {
        value: i32,
    }

    impl HostModule for ValueHost {
        fn name(&self) -> &str {
            "shared"
        }

        fn build(&self, exports: &mut Exports) {
            exports.constant("value", self.value);
        }
    }

    struct FirstNative;

    impl ModuleDef for FirstNative {
        fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
            declarations.declare("value")?;

            Ok(())
        }

        fn evaluate<'js>(_ctx: &Ctx<'js>, exports: &JsExports<'js>) -> rquickjs::Result<()> {
            exports.export("value", 1_i32)?;

            Ok(())
        }
    }

    struct SecondNative;

    impl ModuleDef for SecondNative {
        fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
            declarations.declare("value")?;

            Ok(())
        }

        fn evaluate<'js>(_ctx: &Ctx<'js>, exports: &JsExports<'js>) -> rquickjs::Result<()> {
            exports.export("value", 2_i32)?;

            Ok(())
        }
    }

    struct TrackedHostModule {
        name: String,
        value: i32,
        drops: Arc<AtomicUsize>,
    }

    impl TrackedHostModule {
        fn new(name: impl Into<String>, value: i32, drops: Arc<AtomicUsize>) -> Self {
            Self { name: name.into(), value, drops }
        }
    }

    impl Drop for TrackedHostModule {
        fn drop(&mut self) {
            self.drops
                .fetch_add(1, Ordering::SeqCst);
        }
    }

    impl HostModule for TrackedHostModule {
        fn name(&self) -> &str {
            &self.name
        }

        fn build(&self, exports: &mut Exports) {
            exports.default(self.value);
        }
    }

    #[tokio::test]
    async fn eval_returns_value() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<i32>("1 + 1")
                .await
                .unwrap(),
            2,
        );
    }

    #[tokio::test]
    async fn execution_timeout_unwinds_an_infinite_loop() {
        assert!(matches!(
            Runtime::builder()
                .execution_timeout(Duration::from_millis(50))
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<()>("while (true) {}")
                .await,
            Err(Error::Timeout),
        ));
    }

    #[tokio::test]
    async fn a_precancelled_runtime_refuses_to_execute() {
        let cancellation = Cancellation::new();
        let guest = Runtime::builder()
            .cancellation(cancellation.clone())
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        cancellation.cancel();

        assert!(matches!(
            guest
                .eval::<i32>("1 + 1")
                .await,
            Err(Error::Cancelled),
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_from_another_thread_interrupts_execution() {
        let cancellation = Cancellation::new();
        let guest = Runtime::builder()
            .cancellation(cancellation.clone())
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        let cancellation_task = tokio::spawn({
            let cancellation = cancellation.clone();

            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;

                cancellation.cancel();
            }
        });

        assert!(matches!(
            guest
                .eval::<()>("while (true) {}")
                .await,
            Err(Error::Cancelled),
        ));

        cancellation_task.await.unwrap();
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_tokio_cancellation_token_interrupts_execution() {
        let token = CancellationToken::new();
        let guest = Runtime::builder()
            .cancellation(token.clone())
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        let cancellation_task = tokio::spawn({
            let token = token.clone();

            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;

                token.cancel();
            }
        });

        assert!(matches!(
            guest
                .eval::<()>("while (true) {}")
                .await,
            Err(Error::Cancelled),
        ));

        cancellation_task.await.unwrap();
    }

    #[tokio::test]
    async fn max_stack_size_bounds_deep_recursion() {
        assert!(
            Runtime::builder()
                .max_stack_size(64 * 1024)
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<i32>(
                    "(function recurse(n) { return recurse(n + 1); })(0)",
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn gc_after_collects_without_disturbing_results() {
        let guest = Runtime::builder()
            .gc_after(1)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        for _ in 0..4 {
            assert_eq!(
                guest
                    .eval::<i32>("1 + 1")
                    .await
                    .unwrap(),
                2,
            );
        }
    }

    #[tokio::test]
    async fn a_raw_interrupt_handler_aborts_execution() {
        assert!(matches!(
            Runtime::builder()
                .interrupt_handler(|| true)
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<()>("while (true) {}")
                .await,
            Err(Error::Interrupted),
        ));
    }

    #[tokio::test]
    async fn load_javascript_module() {
        let module = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "greet.js",
                "export const settings = { prefix: 'hi' };\n\
                 export function greet(name) { return `${settings.prefix} ${name}`; }",
            )
            .await
            .unwrap();

        assert_eq!(
            module
                .object("settings")
                .await
                .unwrap()
                .get::<String>("prefix")
                .await
                .unwrap(),
            "hi",
        );

        assert_eq!(
            module
                .function("greet")
                .await
                .unwrap()
                .call::<_, String>(("ada",))
                .await
                .unwrap(),
            "hi ada",
        );
    }

    #[tokio::test]
    async fn loads_guest_module_within_scope() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async move |scope| {
                let module = scope
                    .guest_module("greet.js", SCOPED_GUEST_MODULE_SOURCE)
                    .await?;

                assert_eq!(
                    module
                        .object("settings")?
                        .get::<String>("prefix")?,
                    "hello",
                );
                assert_eq!(
                    module
                        .function("greet")?
                        .call::<_, String>(("ada",))?,
                    "hello ada",
                );

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn loads_registered_host_module_within_scope() {
        let runtime = Runtime::builder()
            .bind(ArithmeticHostModule)
            .build()
            .await
            .unwrap();

        runtime
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async move |scope| {
                assert_eq!(
                    scope
                        .host_module("@host/arithmetic")
                        .await?
                        .function("multiply")?
                        .call::<_, i32>((6, 7))?,
                    42,
                );

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn promotes_scoped_module() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .scope(async move |scope| {
                    scope
                        .guest_module("greet.js", SCOPED_GUEST_MODULE_SOURCE)
                        .await?
                        .into_owned()
                })
                .await
                .unwrap()
                .function("greet")
                .await
                .unwrap()
                .call::<_, String>(("ada",))
                .await
                .unwrap(),
            "hello ada",
        );
    }

    #[tokio::test]
    async fn detached_scope_rejects_module_loading() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async move |scope| {
                let scope = Scope::detached(scope.ctx().clone());

                assert!(matches!(
                    scope
                        .guest_module("broken.js", "export {")
                        .await,
                    Err(Error::Unexpected { message, .. })
                        if message == "cannot build an owned guest handle on detached scope",
                ));
                assert!(matches!(
                    scope
                        .host_module("@host/missing")
                        .await,
                    Err(Error::Unexpected { message, .. })
                        if message == "cannot build an owned guest handle on detached scope",
                ));

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn read_globals_by_name() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        guest
            .eval::<()>("globalThis.tz = 'utc'; globalThis.add = (a, b) => a + b;")
            .await
            .unwrap();

        assert_eq!(
            guest
                .globals()
                .await
                .unwrap()
                .get::<String>("tz")
                .await
                .unwrap(),
            "utc",
        );

        assert_eq!(
            guest
                .globals()
                .await
                .unwrap()
                .get::<Function>("add")
                .await
                .unwrap()
                .call::<_, i32>((2, 3))
                .await
                .unwrap(),
            5,
        );
    }

    #[tokio::test]
    async fn dropping_guest_releases_scoped_module() {
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let guest = runtime
            .guest()
            .bind(TrackedHostModule::new("@host/scoped", 1, drops.clone()))
            .build()
            .await
            .unwrap();

        assert_eq!(drops.load(Ordering::SeqCst), 0);

        drop(guest);

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn owned_handle_delays_scoped_module_cleanup() {
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let guest = runtime
            .guest()
            .bind(TrackedHostModule::new("@host/scoped", 1, drops.clone()))
            .build()
            .await
            .unwrap();

        let globals = guest.globals().await.unwrap();

        drop(guest);

        assert_eq!(drops.load(Ordering::SeqCst), 0);

        drop(globals);

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dropping_guest_preserves_other_guest() {
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let first_drops = Arc::new(AtomicUsize::new(0));
        let second_drops = Arc::new(AtomicUsize::new(0));
        let first = runtime
            .guest()
            .bind(TrackedHostModule::new("@host/first", 1, first_drops.clone()))
            .build()
            .await
            .unwrap();
        let second = runtime
            .guest()
            .bind(TrackedHostModule::new("@host/second", 2, second_drops.clone()))
            .build()
            .await
            .unwrap();

        drop(first);

        assert_eq!(first_drops.load(Ordering::SeqCst), 1);
        assert_eq!(second_drops.load(Ordering::SeqCst), 0);
        assert_eq!(
            second
                .eval::<i32>("1 + 1")
                .await
                .unwrap(),
            2
        );

        drop(second);

        assert_eq!(second_drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn global_module_survives_guest_cleanup() {
        let drops = Arc::new(AtomicUsize::new(0));
        let runtime = Runtime::builder()
            .bind(TrackedHostModule::new("@host/global", 7, drops.clone()))
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .host_module("@host/global")
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            7,
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .host_module("@host/global")
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            7,
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        drop(runtime);

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repeated_guest_batches_release_scoped_modules() {
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let drops = Arc::new(AtomicUsize::new(0));

        for batch in 0..3 {
            let mut guests = Vec::new();

            for index in 0..3 {
                guests.push(
                    runtime
                        .guest()
                        .bind(TrackedHostModule::new(
                            format!("@host/batch-{batch}-{index}"),
                            1,
                            drops.clone(),
                        ))
                        .build()
                        .await
                        .unwrap(),
                );
            }

            assert_eq!(drops.load(Ordering::SeqCst), batch * 3);

            drop(guests);

            assert_eq!(drops.load(Ordering::SeqCst), (batch + 1) * 3);
        }
    }

    #[tokio::test]
    async fn global_native_module_is_available_to_every_guest() {
        let runtime = Runtime::builder()
            .bind_native(NativeModule::new("shared", FirstNative))
            .build()
            .await
            .unwrap();

        for name in ["first.js", "second.js"] {
            assert_eq!(
                runtime
                    .guest()
                    .build()
                    .await
                    .unwrap()
                    .guest_module(name, IMPORT_SHARED_VALUE_SOURCE)
                    .await
                    .unwrap()
                    .get::<i32>("default")
                    .await
                    .unwrap(),
                1,
            );
        }
    }

    #[tokio::test]
    async fn builders_accept_multiple_modules_per_library() {
        let runtime = Runtime::builder()
            .bind(
                HostLibrary::new()
                    .with(ArithmeticHostModule)
                    .with(ValueHost { value: 7 }),
            )
            .bind_native(
                NativeLibrary::new()
                    .with(NativeModule::new("first", FirstNative))
                    .with(NativeModule::new("second", SecondNative)),
            )
            .build()
            .await
            .unwrap();
        let guest = runtime.guest().build().await.unwrap();

        assert_eq!(
            guest
                .host_module("@host/arithmetic")
                .await
                .unwrap()
                .function("multiply")
                .await
                .unwrap()
                .call::<_, i32>((6, 7))
                .await
                .unwrap(),
            42,
        );
        assert_eq!(
            guest
                .guest_module("multiple.js", IMPORT_MULTIPLE_NATIVE_SOURCE)
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            3,
        );
    }

    #[tokio::test]
    async fn local_native_module_is_isolated() {
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let privileged = runtime
            .guest()
            .bind_native(NativeModule::new("shared", FirstNative))
            .build()
            .await
            .unwrap();
        let restricted = runtime.guest().build().await.unwrap();

        assert_eq!(
            privileged
                .guest_module("privileged.js", IMPORT_SHARED_VALUE_SOURCE)
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            1,
        );
        assert!(
            restricted
                .guest_module("restricted.js", IMPORT_SHARED_VALUE_SOURCE)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn local_host_replaces_global_native_module() {
        let runtime = Runtime::builder()
            .bind_native(NativeModule::new("shared", FirstNative))
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .bind(ValueHost { value: 7 })
                .build()
                .await
                .unwrap()
                .guest_module("host-wins.js", IMPORT_SHARED_VALUE_SOURCE)
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            7,
        );
    }

    #[tokio::test]
    async fn local_native_module_replaces_global_host() {
        let runtime = Runtime::builder()
            .bind(ValueHost { value: 7 })
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .bind_native(NativeModule::new("shared", SecondNative))
                .build()
                .await
                .unwrap()
                .guest_module("native-wins.js", IMPORT_SHARED_VALUE_SOURCE)
                .await
                .unwrap()
                .get::<i32>("default")
                .await
                .unwrap(),
            2,
        );
    }

    #[tokio::test]
    async fn repeated_guests_load_reusable_native_module() {
        let runtime = Runtime::builder()
            .bind_native(NativeModule::new("shared", FirstNative))
            .build()
            .await
            .unwrap();

        for index in 0..3 {
            assert_eq!(
                runtime
                    .guest()
                    .build()
                    .await
                    .unwrap()
                    .guest_module(&format!("batch-{index}.js"), IMPORT_SHARED_VALUE_SOURCE,)
                    .await
                    .unwrap()
                    .get::<i32>("default")
                    .await
                    .unwrap(),
                1,
            );
        }
    }

    #[tokio::test]
    async fn native_initializer_runs_once_per_guest() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Runtime::builder()
            .bind_native(NativeLibrary::new().initialize(NativeInitializer::new("test:init", {
                let calls = calls.clone();

                move |_ctx| {
                    calls.fetch_add(1, Ordering::SeqCst);

                    Ok(())
                }
            })))
            .build()
            .await
            .unwrap();

        runtime.guest().build().await.unwrap();
        runtime.guest().build().await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn initializer_failure_cleans_up_guest_registration() {
        let drops = Arc::new(AtomicUsize::new(0));
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();

        assert!(
            runtime
                .guest()
                .bind(TrackedHostModule::new("shared", 1, drops.clone(),))
                .bind_native(
                    NativeLibrary::new()
                        .initialize(NativeInitializer::new("test:failure", |_ctx| Err(
                            JsError::Unknown
                        ),))
                )
                .build()
                .await
                .is_err()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "typescript")]
    #[tokio::test]
    async fn load_typescript_module() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "greet.ts",
                    "export function greet(name: string): string { return `hi ${name}`; }",
                )
                .await
                .unwrap()
                .function("greet")
                .await
                .unwrap()
                .call::<_, String>(("ada",))
                .await
                .unwrap(),
            "hi ada",
        );
    }

    #[cfg(feature = "typescript")]
    #[tokio::test]
    async fn loads_typescript_module_within_scope() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async move |scope| {
                assert_eq!(
                    scope
                        .guest_module("scoped-color.ts", SCOPED_TYPESCRIPT_MODULE_SOURCE,)
                        .await?
                        .function("pick")?
                        .call::<_, i32>(())?,
                    2,
                );

                Ok(())
            })
            .await
            .unwrap();
    }

    #[cfg(feature = "typescript")]
    #[tokio::test]
    async fn transpiles_typescript_enum() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "color.ts",
                    "enum Color { Red, Green, Blue }\n\
                     export function pick() { return Color.Blue; }",
                )
                .await
                .unwrap()
                .function("pick")
                .await
                .unwrap()
                .call::<_, i32>(())
                .await
                .unwrap(),
            2,
        );
    }

    #[cfg(feature = "typescript")]
    #[tokio::test]
    async fn javascript_passes_through() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("plain.js", "export function add(a, b) { return a + b; }")
                .await
                .unwrap()
                .function("add")
                .await
                .unwrap()
                .call::<_, i32>((2, 3))
                .await
                .unwrap(),
            5,
        );
    }

    #[cfg(feature = "typescript")]
    #[tokio::test]
    async fn rejects_invalid_typescript() {
        assert!(matches!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("bad.ts", "export function oops(: {")
                .await,
            Err(Error::Transpile { .. }),
        ));
    }
}
