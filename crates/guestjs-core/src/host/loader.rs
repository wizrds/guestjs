use std::sync::Arc;

use rquickjs::{
    loader::{
        ImportAttributes,
        Loader,
        Resolver,
    },
    module::{
        Declarations,
        Exports as ModuleExports,
        ModuleDef,
    },
    Ctx,
    Error as JsError,
    Exception,
    Module as JsModule,
    Result as JsResult,
};

use crate::{
    host::{
        module::Exports,
        registry::{
            GuestId,
            HostRegistry,
            RegistryHandle,
        },
    },
    runtime::Scope,
};

/// The module adapter for host-defined modules.
pub(crate) struct HostModuleAdapter;

impl HostModuleAdapter {
    /// Fetches the shared registry stored on the runtime as userdata.
    fn registry(ctx: &Ctx<'_>) -> JsResult<Arc<HostRegistry>> {
        Ok(
            ctx.userdata::<RegistryHandle>()
                .ok_or_else(|| {
                    Exception::throw_message(ctx, "host registry is not installed")
                })?
                .0
                .clone(),
        )
    }

    fn guest_id(registry: &HostRegistry, ctx: &Ctx<'_>) -> JsResult<GuestId> {
        registry.guest_id(ctx).ok_or_else(|| {
            Exception::throw_message(ctx, "guest context is not registered")
        })
    }
}

impl ModuleDef for HostModuleAdapter {
    fn declare<'js>(declarations: &Declarations<'js>) -> JsResult<()> {
        let ctx = declarations.module().ctx().clone();
        let name = declarations.module().name::<String>()?;

        let registry = Self::registry(&ctx)?;
        let guest = Self::guest_id(&registry, &ctx)?;

        let mut exports = Exports::new();

        registry
            .lookup(guest, &name)
            .ok_or_else(|| {
                Exception::throw_message(
                    &ctx,
                    &format!("no host module registered as {name:?}"),
                )
            })?
            .build(&mut exports);

        let namespace = exports.into_namespace();

        for (export, _member) in namespace.members() {
            declarations.declare(export.as_str())?;
        }

        registry.stage(guest, name, namespace);

        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &ModuleExports<'js>) -> JsResult<()> {
        let name = exports.module().name::<String>()?;

        let registry = Self::registry(ctx)?;
        let guest = Self::guest_id(&registry, ctx)?;

        let namespace = registry.take_staged(guest, &name).ok_or_else(|| {
            Exception::throw_message(ctx, &format!("no staged exports for host module {name:?}"))
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

/// A resolver for registered host-module specifiers.
pub(crate) struct HostResolver {
    registry: Arc<HostRegistry>,
}

impl HostResolver {
    pub(crate) fn new(registry: Arc<HostRegistry>) -> Self {
        Self { registry }
    }
}

impl Resolver for HostResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<String> {
        let guest = self.registry.guest_id(ctx).ok_or_else(|| {
            Exception::throw_message(ctx, "guest context is not registered")
        })?;

        if self.registry.is_known(guest, name) {
            Ok(name.to_owned())
        } else {
            Err(JsError::new_resolving(base, name))
        }
    }
}

/// A loader for registered host modules.
pub(crate) struct HostLoader;

impl Loader for HostLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<JsModule<'js>> {
        JsModule::declare_def::<HostModuleAdapter, _>(ctx.clone(), name.to_owned())
    }
}
