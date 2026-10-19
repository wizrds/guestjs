use std::rc::Rc;

use crate::{
    backend::Backend,
    enter::Enter,
    errors::Error,
    host::module::ModuleDeclaration,
};

pub struct HostInitializer<B: Backend> {
    initialize: Rc<dyn for<'js> Fn(&Enter<'js, B>) -> Result<(), Error>>,
}

impl<B: Backend> HostInitializer<B> {
    pub fn new<F>(initialize: F) -> Self
    where
        F: for<'js> Fn(&Enter<'js, B>) -> Result<(), Error> + 'static,
    {
        Self {
            initialize: Rc::new(initialize),
        }
    }

    pub fn run<'js>(&self, enter: &Enter<'js, B>) -> Result<(), Error> {
        (self.initialize)(enter)
    }
}

impl<B: Backend> Clone for HostInitializer<B> {
    fn clone(&self) -> Self {
        Self {
            initialize: self.initialize.clone(),
        }
    }
}

pub struct HostLibrary<B: Backend> {
    modules: Vec<Rc<ModuleDeclaration<B>>>,
    initializers: Vec<HostInitializer<B>>,
}

impl<B: Backend> HostLibrary<B> {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            initializers: Vec::new(),
        }
    }

    pub fn extend<L>(mut self, library: L) -> Self
    where
        L: Into<HostLibrary<B>>,
    {
        let library = library.into();

        self.modules.extend(library.modules);
        self.initializers.extend(library.initializers);

        self
    }

    pub fn modules(&self) -> &[Rc<ModuleDeclaration<B>>] {
        &self.modules
    }

    pub fn initializers(&self) -> &[HostInitializer<B>] {
        &self.initializers
    }
}

impl<B: Backend> Default for HostLibrary<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> Clone for HostLibrary<B> {
    fn clone(&self) -> Self {
        Self {
            modules: self.modules.clone(),
            initializers: self.initializers.clone(),
        }
    }
}

impl<B: Backend> From<ModuleDeclaration<B>> for HostLibrary<B> {
    fn from(module: ModuleDeclaration<B>) -> Self {
        Self {
            modules: vec![Rc::new(module)],
            initializers: Vec::new(),
        }
    }
}

impl<B: Backend> From<HostInitializer<B>> for HostLibrary<B> {
    fn from(initializer: HostInitializer<B>) -> Self {
        Self {
            modules: Vec::new(),
            initializers: vec![initializer],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        rc::Rc,
    };

    use crate::{
        backend::tests::Stub,
        guest::Guest,
        host::{
            library::{
                HostInitializer,
                HostLibrary,
            },
            module::ModuleDeclaration,
        },
        runtime::Runtime,
    };

    #[test]
    fn a_module_converts_into_a_library() {
        let library = HostLibrary::<Stub>::from(ModuleDeclaration::new("fs"));

        assert_eq!(library.modules()[0].name(), "fs");
        assert!(library.initializers().is_empty());
    }

    #[test]
    fn extending_keeps_each_list_in_order() {
        let library = HostLibrary::<Stub>::new()
            .extend(ModuleDeclaration::new("a"))
            .extend(HostInitializer::new(|_| Ok(())))
            .extend(
                HostLibrary::new()
                    .extend(ModuleDeclaration::new("b"))
                    .extend(HostInitializer::new(|_| Ok(()))),
            );

        assert_eq!(
            library
                .modules()
                .iter()
                .map(|module| module.name())
                .collect::<Vec<_>>(),
            ["a", "b"],
        );
        assert_eq!(library.initializers().len(), 2);
    }

    #[test]
    fn clones_share_their_modules() {
        let library = HostLibrary::<Stub>::from(ModuleDeclaration::new("fs"));

        assert!(Rc::ptr_eq(
            &library.modules()[0],
            &library.clone().modules()[0],
        ));
    }

    #[test]
    fn an_initializer_runs_against_the_guest_it_is_given() {
        let guest = Guest::new(&Runtime::<Stub>::new(()).unwrap()).unwrap();
        let seen = Rc::new(Cell::new(None));

        guest
            .enter(|enter| {
                HostInitializer::new({
                    let seen = seen.clone();

                    move |enter| {
                        seen.set(Some(enter.guest().id()));

                        Ok(())
                    }
                })
                .run(enter)
            })
            .unwrap();

        assert_eq!(seen.get(), Some(guest.id()));
    }
}
