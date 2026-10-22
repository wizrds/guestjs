use crate::{
    backend::{
        callables::BackendCallables, classes::BackendClasses, exceptions::BackendExceptions,
        interrupt::BackendInterrupt, modules::BackendModules, promises::BackendPromises,
    },
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::{Error, GuestErrorKind},
    eval::{GuestEvalExt, GuestGlobalsExt},
    fixtures::guest_fixture,
    guest::Guest,
    handle::{HasHandle, class::Class, protocol::ObjectProtocol},
    host::{exception::HostException, module::ModuleDeclaration},
    imports::GuestModulesExt,
    marshal::ToGuest,
    runtime::Runtime,
};

struct Boom;

impl HostException for Boom {
    const NAME: &'static str = "Boom";
}

struct InvalidInput;

impl HostException for InvalidInput {
    const NAME: &'static str = "InvalidInput";
    const BASE: GuestErrorKind = GuestErrorKind::TypeError;
}

struct BuiltinErrors;

impl BuiltinErrors {
    fn module<B>() -> ModuleDeclaration<B>
    where
        B: BackendCallables + BackendExceptions,
    {
        ModuleDeclaration::new("builtin_errors").function("trigger", |context, _| {
            Err::<(), Error>(B::throw(
                context.enter().token(),
                B::new_error(context.enter().token(), GuestErrorKind::TypeError, "bad value")?,
            ))
        })
    }
}

struct NestedErrors;

impl NestedErrors {
    fn module<B>() -> ModuleDeclaration<B>
    where
        B: BackendCallables + BackendExceptions,
    {
        ModuleDeclaration::new("nested_errors").function("trigger", |context, _| {
            Err::<(), Error>(B::throw(
                context.enter().token(),
                context
                    .enter()
                    .guest()
                    .globals()?
                    .object("Outer")?
                    .class("Inner")?
                    .construct(("nested",))?
                    .to_guest(context.enter())?,
            ))
        })
    }
}

struct Errors;

impl Errors {
    fn module<B>(name: &str) -> ModuleDeclaration<B>
    where
        B: BackendClasses + BackendExceptions,
    {
        ModuleDeclaration::new(name)
            .exception::<Boom>()
            .exception::<InvalidInput>()
            .function("trigger", |context, _| {
                Err::<(), Error>(Boom::throw(context.enter(), "bad value"))
            })
            .function("invalid", |context, _| {
                Err::<(), Error>(InvalidInput::throw(context.enter(), "invalid input"))
            })
    }

    fn install<B>(guest: &Guest<B>, name: &str)
    where
        B: BackendModules + BackendPromises + BackendExceptions,
    {
        guest
            .globals()
            .unwrap()
            .set(name, guest.host_module(name).unwrap())
            .unwrap();
    }

    fn constructor<B>(guest: &Guest<B>, name: &str) -> Class<B>
    where
        B: BackendModules + BackendPromises + BackendExceptions,
    {
        guest
            .host_module(name)
            .unwrap()
            .class("Boom")
            .unwrap()
    }
}

guest_fixture! {
    pub fn host_callback_throws_a_builtin_error_with_its_message<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(BuiltinErrors::module());
    |guest| {
        Errors::install(&guest, "builtin_errors");

        assert!(matches!(
            guest.exec("builtin_errors.trigger();"),
            Err(Error::Guest(exception))
                if exception.matches("TypeError")
                    && exception.message() == "bad value",
        ));
    }
}

guest_fixture! {
    pub fn raises_a_declared_host_exception_class_with_stable_identity<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Errors::module("errors"));
    |guest| {
        Errors::install(&guest, "errors");

        guest
            .exec(
                r#"
        globalThis.first = null;
        globalThis.second = null;

        try {
            errors.trigger();
        } catch (error) {
            first = error.constructor;
        }

        try {
            errors.trigger();
        } catch (error) {
            second = error.constructor;
        }
        "#,
            )
            .unwrap();

        assert!(guest
            .eval::<bool>(
                "first === errors.Boom && second === errors.Boom && first === second",
            )
            .unwrap());

        let error = guest.exec("errors.trigger();").unwrap_err();

        assert!(Boom::caught(&error));
        assert!(matches!(
            &error,
            Error::Guest(exception) if exception.message() == "bad value",
        ));
    }
}

guest_fixture! {
    pub fn host_callback_throws_a_nested_guest_exception<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(NestedErrors::module());
    |guest| {
        Errors::install(&guest, "nested_errors");

        guest
            .exec(
                r#"
        globalThis.Outer = { Inner: class Inner extends Error {} };
        globalThis.caught = false;

        try {
            nested_errors.trigger();
        } catch (error) {
            if (!(error instanceof Outer.Inner)) {
                throw error;
            }

            caught = error.message === "nested";
        }
        "#,
            )
            .unwrap();

        assert!(guest.eval::<bool>("caught").unwrap());
    }
}

guest_fixture! {
    pub fn typed_exception_names_match_without_public_entry<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Errors::module("errors"));
    |guest| {
        Errors::install(&guest, "errors");

        let error = guest.exec("errors.invalid();").unwrap_err();

        assert!(InvalidInput::caught(&error));
        assert!(!Boom::caught(&error));
        assert!(matches!(
            &error,
            Error::Guest(exception)
                if exception.matches("TypeError")
                    && exception.message() == "invalid input",
        ));
        assert!(!InvalidInput::caught(&Error::closed()));
    }
}

guest_fixture! {
    pub fn the_first_registration_controls_guest_exception_identity<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder()
        .bind(Errors::module("first_errors"))
        .bind(Errors::module("second_errors"));
    |guest| {
        Errors::install(&guest, "first_errors");
        Errors::install(&guest, "second_errors");

        assert!(guest
            .eval::<bool>("first_errors.Boom === second_errors.Boom")
            .unwrap());

        guest
            .exec(
                r#"
        globalThis.caught = null;

        try {
            second_errors.trigger();
        } catch (error) {
            caught = error;
        }
        "#,
            )
            .unwrap();

        assert!(guest
            .eval::<bool>(
                "caught instanceof first_errors.Boom && caught.constructor === first_errors.Boom",
            )
            .unwrap());
    }
}

pub fn exception_classes_are_local_to_guests_and_runtimes<B>()
where
    B: BackendModules + BackendInterrupt + BackendClasses + BackendPromises + BackendExceptions,
{
    let runtime = Runtime::<B>::builder()
        .bind(Errors::module("errors"))
        .build()
        .unwrap();
    let first = runtime.guest().build().unwrap();
    let second = runtime.guest().build().unwrap();
    let third = Runtime::<B>::builder()
        .bind(Errors::module("errors"))
        .build()
        .unwrap()
        .guest()
        .build()
        .unwrap();
    let first_class = Errors::constructor(&first, "errors");
    let second_class = Errors::constructor(&second, "errors");
    let third_class = Errors::constructor(&third, "errors");

    assert!(
        first_class
            .handle()
            .same(Errors::constructor(&first, "errors").handle())
    );
    assert!(
        !first_class
            .handle()
            .same(second_class.handle())
    );
    assert!(
        !first_class
            .handle()
            .same(third_class.handle())
    );

    first_class.set("marker", 1i64).unwrap();

    assert!(!second_class.has("marker").unwrap());
    assert!(!third_class.has("marker").unwrap());
    assert!(
        second
            .enter(|enter| first_class
                .clone()
                .to_guest(enter)
                .map(drop))
            .is_err_and(|error| error.is_foreign_context())
    );
    assert!(
        third
            .enter(|enter| first_class.to_guest(enter).map(drop))
            .is_err_and(|error| error.is_foreign_context())
    );
}

#[macro_export]
macro_rules! __guestjs_backend_exceptions_tests {
    ($backend:ty) => {
        #[test]
        fn host_callback_throws_a_builtin_error_with_its_message() {
            $crate::fixtures::exceptions::host_callback_throws_a_builtin_error_with_its_message::<$backend>();
        }

        #[test]
        fn raises_a_declared_host_exception_class_with_stable_identity() {
            $crate::fixtures::exceptions::raises_a_declared_host_exception_class_with_stable_identity::<$backend>();
        }

        #[test]
        fn host_callback_throws_a_nested_guest_exception() {
            $crate::fixtures::exceptions::host_callback_throws_a_nested_guest_exception::<$backend>();
        }

        #[test]
        fn typed_exception_names_match_without_public_entry() {
            $crate::fixtures::exceptions::typed_exception_names_match_without_public_entry::<$backend>();
        }

        #[test]
        fn the_first_registration_controls_guest_exception_identity() {
            $crate::fixtures::exceptions::the_first_registration_controls_guest_exception_identity::<$backend>();
        }

        #[test]
        fn exception_classes_are_local_to_guests_and_runtimes() {
            $crate::fixtures::exceptions::exception_classes_are_local_to_guests_and_runtimes::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_exceptions_tests as tests;
