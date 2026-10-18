use std::sync::Arc;

use rquickjs::{CatchResultExt, Ctx, Result as JsResult};

use crate::errors::Error;

type InitializeNative = dyn for<'js> Fn(&Ctx<'js>) -> JsResult<()>;

/// A reusable native context initializer.
#[derive(Clone)]
pub struct NativeInitializer {
    name: String,
    initialize: Arc<InitializeNative>,
}

impl NativeInitializer {
    /// Creates a native context initializer.
    pub fn new<N, F>(name: N, initialize: F) -> Self
    where
        N: Into<String>,
        F: for<'js> Fn(&Ctx<'js>) -> JsResult<()> + 'static,
    {
        Self {
            name: name.into(),
            initialize: Arc::new(initialize),
        }
    }

    /// Returns the initializer name.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn initialize<'js>(&self, ctx: &Ctx<'js>) -> Result<(), Error> {
        (self.initialize)(ctx).catch(ctx)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use rquickjs::{Context as JsContext, Runtime as JsRuntime};

    use super::NativeInitializer;

    #[test]
    fn reuses_captured_initializer_across_contexts() {
        let calls = Rc::new(Cell::new(0));
        let initializer = NativeInitializer::new("provider:setup", {
            let calls = calls.clone();

            move |_ctx| {
                calls.set(calls.get() + 1);

                Ok(())
            }
        });
        let runtime = JsRuntime::new().unwrap();
        let first = JsContext::full(&runtime).unwrap();
        let second = JsContext::full(&runtime).unwrap();

        first.with(|ctx| initializer.initialize(&ctx).unwrap());
        second.with(|ctx| initializer.initialize(&ctx).unwrap());

        assert_eq!(initializer.name(), "provider:setup");
        assert_eq!(calls.get(), 2);
    }
}
