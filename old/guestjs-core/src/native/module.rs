use rquickjs::{Ctx, Module as JsModule, Result as JsResult, module::ModuleDef};

use crate::native::NativeInitializer;

type DeclareNative = for<'js> fn(Ctx<'js>, Vec<u8>) -> JsResult<JsModule<'js>>;

/// A reusable native guest module.
#[derive(Clone)]
pub struct NativeModule {
    name: String,
    aliases: Vec<String>,
    declare: DeclareNative,
    initializers: Vec<NativeInitializer>,
}

impl NativeModule {
    /// Creates a native guest module.
    pub fn new<N, M>(name: N, _module: M) -> Self
    where
        N: Into<String>,
        M: ModuleDef + 'static,
    {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            declare: Self::declare_def::<M>,
            initializers: Vec::new(),
        }
    }

    fn declare_def<'js, M>(ctx: Ctx<'js>, name: Vec<u8>) -> JsResult<JsModule<'js>>
    where
        M: ModuleDef,
    {
        JsModule::declare_def::<M, Vec<u8>>(ctx, name)
    }

    /// Adds a module specifier alias.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        let alias = alias.into();

        if alias != self.name && !self.aliases.contains(&alias) {
            self.aliases.push(alias);
        }

        self
    }

    /// Adds a [`NativeInitializer`](crate::native::NativeInitializer).
    pub fn initialize(mut self, initializer: NativeInitializer) -> Self {
        self.initializers.push(initializer);

        self
    }

    /// Returns the canonical module name.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub(crate) fn initializers(&self) -> &[NativeInitializer] {
        &self.initializers
    }

    pub(crate) fn declare<'js>(&self, ctx: Ctx<'js>, name: Vec<u8>) -> JsResult<JsModule<'js>> {
        (self.declare)(ctx, name)
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context as JsContext, Runtime as JsRuntime, module::ModuleDef};

    use super::NativeModule;
    use crate::native::NativeInitializer;

    struct TestModule;

    impl ModuleDef for TestModule {}

    #[test]
    fn retains_module_metadata() {
        let module = NativeModule::new("test", TestModule)
            .alias("test")
            .alias("node:test")
            .alias("node:test")
            .alias("test/alias")
            .initialize(NativeInitializer::new("test:init", |_ctx| Ok(())));

        assert_eq!(module.name(), "test");
        assert_eq!(module.aliases(), ["node:test", "test/alias"]);
        assert_eq!(module.initializers()[0].name(), "test:init");
    }

    #[test]
    fn declares_module_in_distinct_contexts() {
        let module = NativeModule::new("test", TestModule);
        let runtime = JsRuntime::new().unwrap();
        let first = JsContext::full(&runtime).unwrap();
        let second = JsContext::full(&runtime).unwrap();

        first.with(|ctx| {
            module
                .declare(ctx, b"test:first".to_vec())
                .unwrap();
        });
        second.with(|ctx| {
            module
                .declare(ctx, b"test:second".to_vec())
                .unwrap();
        });
    }
}
