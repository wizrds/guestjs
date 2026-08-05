use std::sync::Arc;

use crate::host::{HostInitializer, HostModule};

#[derive(Clone)]
pub(crate) enum HostLibraryEntry {
    Module(Arc<dyn HostModule>),
    Initializer(HostInitializer),
}

/// A collection of host guest capabilities.
#[derive(Clone, Default)]
pub struct HostLibrary {
    entries: Vec<HostLibraryEntry>,
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
        self.entries
            .push(HostLibraryEntry::Module(Arc::new(module)));

        self
    }

    /// Adds a [`HostInitializer`](crate::host::HostInitializer).
    pub fn initialize(mut self, initializer: HostInitializer) -> Self {
        self.entries
            .push(HostLibraryEntry::Initializer(initializer));

        self
    }

    pub(crate) fn into_entries(self) -> Vec<HostLibraryEntry> {
        self.entries
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

    use crate::host::{Exports, HostInitializer, HostLibrary, HostLibraryEntry, HostModule};

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
        match HostLibrary::from(FirstHost)
            .into_entries()
            .remove(0)
        {
            HostLibraryEntry::Module(module) => {
                assert_eq!(module.name(), "first");
            }
            HostLibraryEntry::Initializer(_) => {
                panic!("expected a host module");
            }
        }
    }

    #[test]
    fn preserves_heterogeneous_entry_order() {
        let entries = HostLibrary::new()
            .with(FirstHost)
            .initialize(HostInitializer::new("first:init", |_scope| Ok(())))
            .with(SecondHost)
            .into_entries();

        assert!(matches!(
            &entries[0],
            HostLibraryEntry::Module(module) if module.name() == "first"
        ));
        assert!(matches!(
            &entries[1],
            HostLibraryEntry::Initializer(initializer)
                if initializer.name() == "first:init"
        ));
        assert!(matches!(
            &entries[2],
            HostLibraryEntry::Module(module) if module.name() == "second"
        ));

        let first = match &entries[0] {
            HostLibraryEntry::Module(module) => module,
            HostLibraryEntry::Initializer(_) => {
                panic!("expected a host module");
            }
        };
        let second = match &entries[2] {
            HostLibraryEntry::Module(module) => module,
            HostLibraryEntry::Initializer(_) => {
                panic!("expected a host module");
            }
        };

        assert!(!Arc::ptr_eq(first, second));
    }

    #[test]
    fn preserves_initializer_entry() {
        match HostLibrary::new()
            .initialize(HostInitializer::new("provider:init", |_scope| Ok(())))
            .into_entries()
            .remove(0)
        {
            HostLibraryEntry::Module(_) => {
                panic!("expected a host initializer");
            }
            HostLibraryEntry::Initializer(initializer) => {
                assert_eq!(initializer.name(), "provider:init");
            }
        }
    }
}
