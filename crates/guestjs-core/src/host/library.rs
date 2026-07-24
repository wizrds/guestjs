use std::sync::Arc;

use crate::host::HostModule;

/// A collection of [`HostModule`](crate::host::HostModule) implementations.
#[derive(Clone, Default)]
pub struct HostLibrary {
    modules: Vec<Arc<dyn HostModule>>,
}

impl HostLibrary {
    /// Creates an empty host library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a host module.
    pub fn with<M>(mut self, module: M) -> Self
    where
        M: HostModule + 'static,
    {
        self.modules.push(Arc::new(module));

        self
    }

    pub(crate) fn into_modules(self) -> Vec<Arc<dyn HostModule>> {
        self.modules
    }
}

impl<M> From<M> for HostLibrary
where
    M: HostModule + 'static,
{
    fn from(module: M) -> Self {
        Self::new().with(module)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::HostLibrary;
    use crate::host::{Exports, HostModule};

    struct FirstHost;

    impl HostModule for FirstHost {
        fn name(&self) -> &str {
            "first"
        }

        fn build(&self, _exports: &mut Exports) {}
    }

    struct SecondHost;

    impl HostModule for SecondHost {
        fn name(&self) -> &str {
            "second"
        }

        fn build(&self, _exports: &mut Exports) {}
    }

    #[test]
    fn converts_host_module_into_library() {
        assert_eq!(HostLibrary::from(FirstHost).into_modules()[0].name(), "first",);
    }

    #[test]
    fn preserves_heterogeneous_module_order() {
        let modules = HostLibrary::new()
            .with(FirstHost)
            .with(SecondHost)
            .into_modules();

        assert_eq!(
            modules
                .iter()
                .map(|module| module.name())
                .collect::<Vec<_>>(),
            vec!["first", "second"],
        );
        assert!(!Arc::ptr_eq(&modules[0], &modules[1]));
    }
}
