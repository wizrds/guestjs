use crate::native::{NativeInitializer, NativeModule};

#[derive(Clone)]
pub(crate) enum NativeLibraryEntry {
    Module(NativeModule),
    Initializer(NativeInitializer),
}

/// A collection of native guest capabilities.
#[derive(Clone, Default)]
pub struct NativeLibrary {
    entries: Vec<NativeLibraryEntry>,
}

impl NativeLibrary {
    /// Creates an empty native library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a [`NativeModule`](crate::native::NativeModule).
    pub fn with(mut self, module: NativeModule) -> Self {
        self.entries
            .push(NativeLibraryEntry::Module(module));

        self
    }

    /// Adds a [`NativeInitializer`](crate::native::NativeInitializer).
    pub fn initialize(mut self, initializer: NativeInitializer) -> Self {
        self.entries
            .push(NativeLibraryEntry::Initializer(initializer));

        self
    }

    /// Adds the entries from another native library.
    pub fn extend(mut self, library: impl Into<NativeLibrary>) -> Self {
        self.entries
            .extend(library.into().into_entries());

        self
    }

    pub(crate) fn into_entries(self) -> Vec<NativeLibraryEntry> {
        self.entries
    }
}

impl From<NativeModule> for NativeLibrary {
    fn from(module: NativeModule) -> Self {
        Self::new().with(module)
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::module::ModuleDef;

    use super::{NativeLibrary, NativeLibraryEntry};
    use crate::native::{NativeInitializer, NativeModule};

    struct FirstModule;

    impl ModuleDef for FirstModule {}

    struct SecondModule;

    impl ModuleDef for SecondModule {}

    #[test]
    fn converts_native_module_into_library() {
        match NativeLibrary::from(NativeModule::new("first", FirstModule))
            .into_entries()
            .remove(0)
        {
            NativeLibraryEntry::Module(module) => {
                assert_eq!(module.name(), "first");
            }
            NativeLibraryEntry::Initializer(_) => {
                panic!("expected a native module");
            }
        }
    }

    #[test]
    fn extends_library_in_entry_order() {
        let entries = NativeLibrary::new()
            .with(NativeModule::new("first", FirstModule))
            .initialize(NativeInitializer::new("first:init", |_ctx| Ok(())))
            .extend(
                NativeLibrary::new()
                    .with(NativeModule::new("second", SecondModule))
                    .initialize(NativeInitializer::new("second:init", |_ctx| Ok(()))),
            )
            .into_entries();

        assert!(matches!(
            &entries[0],
            NativeLibraryEntry::Module(module) if module.name() == "first"
        ));
        assert!(matches!(
            &entries[1],
            NativeLibraryEntry::Initializer(initializer)
                if initializer.name() == "first:init"
        ));
        assert!(matches!(
            &entries[2],
            NativeLibraryEntry::Module(module) if module.name() == "second"
        ));
        assert!(matches!(
            &entries[3],
            NativeLibraryEntry::Initializer(initializer)
                if initializer.name() == "second:init"
        ));
    }
}
