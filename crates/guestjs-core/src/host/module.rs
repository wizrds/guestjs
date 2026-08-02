use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};

use rquickjs::{
    Ctx, Exception, Result as JsResult,
    module::{Declarations, Exports as ModuleExports, ModuleDef},
};

use crate::{
    host::namespace::Namespace,
    marshal::ToGuest,
    registry::{ModuleRegistry, RegistryHandle},
    runtime::Scope,
};

/// A Rust module exposed to guest code.
pub trait HostModule {
    /// Returns the module specifier.
    fn name(&self) -> &str;

    /// Defines the module exports.
    fn build(&self, exports: &mut Exports);
}

/// The exports of a host module.
pub struct Exports {
    namespace: Namespace,
}

impl Exports {
    pub(crate) fn new() -> Self {
        Self { namespace: Namespace::new() }
    }

    pub(crate) fn into_namespace(self) -> Namespace {
        self.namespace
    }

    /// Defines the default export.
    pub fn default<V>(&mut self, value: V) -> &mut Self
    where
        V: ToGuest + Clone + 'static,
    {
        self.namespace
            .constant("default", value);

        self
    }
}

impl Deref for Exports {
    type Target = Namespace;

    fn deref(&self) -> &Namespace {
        &self.namespace
    }
}

impl DerefMut for Exports {
    fn deref_mut(&mut self) -> &mut Namespace {
        &mut self.namespace
    }
}

pub(crate) struct HostModuleAdapter;

impl HostModuleAdapter {
    fn registry(ctx: &Ctx<'_>) -> JsResult<Rc<ModuleRegistry>> {
        Ok(ctx
            .userdata::<RegistryHandle>()
            .ok_or_else(|| Exception::throw_message(ctx, "module registry is not installed"))?
            .registry())
    }
}

impl ModuleDef for HostModuleAdapter {
    fn declare<'js>(declarations: &Declarations<'js>) -> JsResult<()> {
        let ctx = declarations.module().ctx().clone();
        let route = declarations.module().name::<String>()?;
        let mut exports = Exports::new();
        let registry = Self::registry(&ctx)?;

        registry
            .host_module(&ctx, &route)
            .ok_or_else(|| {
                Exception::throw_message(
                    &ctx,
                    &format!("no host module registered for route {route:?}"),
                )
            })?
            .build(&mut exports);

        let namespace = exports.into_namespace();

        for (export, _member) in namespace.members() {
            declarations.declare(export.as_str())?;
        }

        registry.stage(&ctx, route, namespace);

        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &ModuleExports<'js>) -> JsResult<()> {
        let route = exports.module().name::<String>()?;
        let namespace = Self::registry(ctx)?
            .take_staged(ctx, &route)
            .ok_or_else(|| {
                Exception::throw_message(
                    ctx,
                    &format!("no staged exports for host module route {route:?}"),
                )
            })?;
        let scope = Scope::detached(ctx.clone());

        for (export, member) in namespace.into_members() {
            exports.export(
                export.as_str(),
                member
                    .into_export_value(&scope)
                    .map_err(|error| Exception::throw_message(ctx, &error.to_string()))?,
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{rc::Rc, sync::Arc};

    use rquickjs::{
        Context as JsContext, Runtime as JsRuntime,
        loader::{Loader, Resolver},
    };

    use super::{Exports, HostModule};
    use crate::{
        handle::Promise,
        registry::{LibraryBinding, ModuleLoader, ModuleRegistry, ModuleResolver, RegistryHandle},
        runtime::Runtime,
    };

    struct TimeHost {
        is_monday: bool,
    }

    impl HostModule for TimeHost {
        fn name(&self) -> &str {
            "@host/time"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("hypot", |scope, args| {
                let (x, y) = (args.get::<f64>(scope, 0)?, args.get::<f64>(scope, 1)?);

                Ok((x * x + y * y).sqrt())
            });

            if self.is_monday {
                exports.function("monday", |_scope, _args| Ok("monday!".to_owned()));
            }
        }
    }

    struct ClockHost;

    impl HostModule for ClockHost {
        fn name(&self) -> &str {
            "@host/clock"
        }

        fn build(&self, exports: &mut Exports) {
            exports.object("clock", |clock| {
                clock.constant("tz", "utc");
                clock.function("now", |_scope, _args| Ok(42_i64));
                clock.async_function("later", |_scope, _args| Ok(async { Ok(7_i64) }));
            });
        }
    }

    struct ValueHost;

    impl HostModule for ValueHost {
        fn name(&self) -> &str {
            "host:value"
        }

        fn build(&self, exports: &mut Exports) {
            exports.constant("value", 42_i32);
        }
    }

    #[test]
    fn host_adapter_builds_registered_exports() {
        let registry =
            Rc::new(ModuleRegistry::new(vec![LibraryBinding::Host(Arc::new(ValueHost))]));
        let runtime = JsRuntime::new().unwrap();
        let context = JsContext::full(&runtime).unwrap();

        context.with(|ctx| {
            ctx.store_userdata(RegistryHandle::new(registry.clone()))
                .unwrap();
            registry
                .register_guest(&ctx, Vec::new())
                .unwrap();

            let (module, promise) = ModuleLoader::new(registry.clone())
                .load(
                    &ctx,
                    &ModuleResolver::new(registry.clone())
                        .resolve(&ctx, "entry", "host:value", None)
                        .unwrap(),
                    None,
                )
                .unwrap()
                .eval()
                .unwrap();

            promise.finish::<()>().unwrap();

            assert_eq!(
                module
                    .namespace()
                    .unwrap()
                    .get::<_, i32>("value")
                    .unwrap(),
                42,
            );
        });
    }

    #[tokio::test]
    async fn import_host_binding() {
        let runtime = Runtime::builder()
            .bind(TimeHost { is_monday: true })
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "uses_host.js",
                    "import { hypot, monday } from \"@host/time\";\n\
                     export function run() { return `${hypot(3, 4)}:${monday()}`; }",
                )
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "5:monday!",
        );
    }

    #[tokio::test]
    async fn host_object_method() {
        let runtime = Runtime::builder()
            .bind(ClockHost)
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "clock.js",
                    "import { clock } from \"@host/clock\";\n\
                     export function run() { return `${clock.tz}:${clock.now()}`; }",
                )
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "utc:42",
        );
    }

    #[tokio::test]
    async fn host_object_async_method() {
        let runtime = Runtime::builder()
            .bind(ClockHost)
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "clock.js",
                    "import { clock } from \"@host/clock\";\n\
                     export async function run() { return await clock.later(); }",
                )
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, Promise<i64>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            7,
        );
    }
}
