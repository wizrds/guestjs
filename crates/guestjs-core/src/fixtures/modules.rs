use crate::{
    backend::{
        callables::BackendCallables, exceptions::BackendExceptions, interrupt::BackendInterrupt,
        modules::BackendModules, promises::BackendPromises,
    },
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    bundle::Bundle,
    errors::Error,
    fixtures::guest_fixture,
    handle::protocol::{CallableProtocol, ObjectProtocol},
    host::module::ModuleDeclaration,
    imports::GuestModulesExt,
    runtime::Runtime,
};

guest_fixture! {
    pub fn import_resolves_a_bound_module<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("probe")
            .function("is_probe", |_, _| Ok::<_, Error>(true)),
    );
    |guest| {
        assert!(guest
            .import("probe")
            .unwrap()
            .function("is_probe")
            .unwrap()
            .call::<_, bool>(())
            .unwrap());
    }
}

guest_fixture! {
    pub fn import_resolves_a_nested_module_to_its_leaf<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bundle(
        &Bundle::builder()
            .module("tools/index.js", "export const root_marker = true;")
            .module(
                "tools/path.js",
                "export function join(left, right) { return left + '/' + right; }",
            )
            .build()
            .unwrap(),
    );
    |guest| {
        let leaf = guest.import("tools/path").unwrap();

        assert_eq!(
            leaf.function("join")
                .unwrap()
                .call::<_, String>(("a", "b"))
                .unwrap(),
            "a/b",
        );
        assert!(!leaf.has("root_marker").unwrap());
    }
}

guest_fixture! {
    pub fn import_keeps_denied_modules_denied<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder()
        .bind(
            ModuleDeclaration::new("restricted")
                .function("available", |_, _| Ok::<_, Error>(true)),
        )
        .deny("restricted");
    |guest| {
        assert!(guest
            .import("restricted")
            .err()
            .unwrap()
            .to_string()
            .contains("denied"));
    }
}

#[macro_export]
macro_rules! __guestjs_backend_modules_tests {
    ($backend:ty) => {
        #[test]
        fn import_resolves_a_bound_module() {
            $crate::fixtures::modules::import_resolves_a_bound_module::<$backend>();
        }

        #[test]
        fn import_resolves_a_nested_module_to_its_leaf() {
            $crate::fixtures::modules::import_resolves_a_nested_module_to_its_leaf::<$backend>();
        }

        #[test]
        fn import_keeps_denied_modules_denied() {
            $crate::fixtures::modules::import_keeps_denied_modules_denied::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_modules_tests as tests;
