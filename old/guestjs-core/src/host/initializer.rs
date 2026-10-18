use std::sync::Arc;

use crate::{errors::Error, runtime::Scope};

type InitializeHost = dyn for<'js> Fn(&Scope<'js>) -> Result<(), Error>;

/// A reusable host context initializer.
#[derive(Clone)]
pub struct HostInitializer {
    name: String,
    initialize: Arc<InitializeHost>,
}

impl HostInitializer {
    /// Creates a host context initializer.
    pub fn new<N, F>(name: N, initialize: F) -> Self
    where
        N: Into<String>,
        F: for<'js> Fn(&Scope<'js>) -> Result<(), Error> + 'static,
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

    pub(crate) fn initialize<'js>(&self, scope: &Scope<'js>) -> Result<(), Error> {
        (self.initialize)(scope)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use rquickjs::{Context as JsContext, Runtime as JsRuntime};

    use crate::{host::HostInitializer, runtime::Scope};

    #[test]
    fn reuses_captured_initializer_across_contexts() {
        let calls = Rc::new(Cell::new(0));
        let initializer = HostInitializer::new("provider:setup", {
            let calls = calls.clone();

            move |_scope| {
                calls.set(calls.get() + 1);

                Ok(())
            }
        });
        let runtime = JsRuntime::new().unwrap();
        let first = JsContext::full(&runtime).unwrap();
        let second = JsContext::full(&runtime).unwrap();

        first.with(|ctx| {
            initializer
                .clone()
                .initialize(&Scope::detached(ctx))
                .unwrap();
        });
        second.with(|ctx| {
            initializer
                .initialize(&Scope::detached(ctx))
                .unwrap();
        });

        assert_eq!(initializer.name(), "provider:setup");
        assert_eq!(calls.get(), 2);
    }
}
