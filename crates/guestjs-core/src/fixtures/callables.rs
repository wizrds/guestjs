use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    backend::{
        callables::BackendCallables, exceptions::BackendExceptions, interrupt::BackendInterrupt,
        modules::BackendModules, promises::BackendPromises,
    },
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::Error,
    eval::{GuestEvalExt, GuestGlobalsExt},
    fixtures::guest_fixture,
    guest::Guest,
    handle::protocol::{CallableProtocol, ObjectProtocol},
    host::{library::HostInitializer, module::ModuleDeclaration},
    imports::GuestModulesExt,
    runtime::Runtime,
};

struct Codec;

impl Codec {
    fn module<B>() -> ModuleDeclaration<B>
    where
        B: BackendCallables,
    {
        ModuleDeclaration::new("codec")
            .function("echo_i64", |context, args| args.next::<i64>(context.enter()))
            .function("echo_u8", |context, args| args.next::<u8>(context.enter()))
            .function("echo_f64", |context, args| args.next::<f64>(context.enter()))
            .function("echo_str", |context, args| args.next::<String>(context.enter()))
            .function("echo_list", |context, args| args.next::<Vec<i64>>(context.enter()))
            .function("echo_pair", |context, args| args.next::<(i64, String)>(context.enter()))
            .function("echo_map", |context, args| {
                args.next::<HashMap<String, i64>>(context.enter())
            })
            .function("echo_opt", |context, args| args.next::<Option<i64>>(context.enter()))
            .function("echo_optional", |context, args| args.next::<Option<String>>(context.enter()))
            .function("echo_required", |context, args| args.next::<String>(context.enter()))
            .function("add", |context, args| {
                Ok::<_, Error>(
                    args.next::<i64>(context.enter())? + args.next::<i64>(context.enter())?,
                )
            })
            .function("boom", |_, _| {
                Err::<i64, Error>(Error::conversion(
                    "successful codec operation",
                    "deliberate failure",
                ))
            })
    }

    fn install<B>(guest: &Guest<B>)
    where
        B: BackendModules + BackendPromises + BackendExceptions,
    {
        guest
            .globals()
            .unwrap()
            .set("codec", guest.host_module("codec").unwrap())
            .unwrap();
    }
}

guest_fixture! {
    pub fn calls_a_host_function_both_ways<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        assert_eq!(guest.eval::<i64>("codec.add(2, 3)").unwrap(), 5);
        assert_eq!(
            guest
                .host_module("codec")
                .unwrap()
                .function("add")
                .unwrap()
                .call::<_, i64>((2i64, 3i64))
                .unwrap(),
            5,
        );
        assert!(matches!(
            guest.eval::<i64>("codec.add(2)"),
            Err(Error::Guest(exception)) if exception.matches("TypeError"),
        ));
    }
}

guest_fixture! {
    pub fn round_trips_integers<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        assert_eq!(
            guest
                .eval::<i64>("codec.echo_i64(-9007199254740991)")
                .unwrap(),
            -9007199254740991,
        );
        assert_eq!(guest.eval::<i64>("codec.echo_u8(255)").unwrap(), 255);

        for source in [
            "codec.echo_u8(300)",
            "codec.echo_u8(-1)",
            "codec.echo_i64('x')",
        ] {
            assert!(matches!(
                guest.eval::<i64>(source),
                Err(Error::Guest(exception)) if exception.matches("TypeError"),
            ));
        }
    }
}

guest_fixture! {
    pub fn round_trips_floats_and_strings<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        assert_eq!(guest.eval::<f64>("codec.echo_f64(1.5)").unwrap(), 1.5);
        assert_eq!(
            guest.eval::<String>("codec.echo_str('héllo')").unwrap(),
            "héllo",
        );
    }
}

guest_fixture! {
    pub fn round_trips_sequences<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        assert_eq!(
            guest.eval::<Vec<i64>>("codec.echo_list([1, 2, 3])").unwrap(),
            vec![1, 2, 3],
        );
        assert_eq!(
            guest.eval::<(i64, String)>("codec.echo_pair([1, 'a'])").unwrap(),
            (1, "a".to_owned()),
        );
    }
}

guest_fixture! {
    pub fn round_trips_mappings_and_optionals<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        assert_eq!(
            guest
                .eval::<HashMap<String, i64>>("codec.echo_map({a: 1})")
                .unwrap(),
            HashMap::from([("a".to_owned(), 1)]),
        );
        assert!(guest.eval::<bool>("codec.echo_opt(null) === null").unwrap());
        assert!(guest
            .eval::<bool>("codec.echo_opt(undefined) === null")
            .unwrap());
        assert_eq!(guest.eval::<i64>("codec.echo_opt(4)").unwrap(), 4);
    }
}

guest_fixture! {
    pub fn an_error_from_a_host_body_is_catchable<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        guest
            .exec(
                r#"
        globalThis.caught = "";
        try {
            codec.boom();
        } catch (error) {
            if (!(error instanceof TypeError)) {
                throw error;
            }

            caught = error.message;
        }
        "#,
            )
            .unwrap();

        assert!(guest
            .eval::<String>("caught")
            .unwrap()
            .contains("deliberate failure"));
        assert!(matches!(
            guest.exec("throw new RangeError('nope');"),
            Err(Error::Guest(exception))
                if exception.matches("RangeError") && exception.message() == "nope",
        ));
    }
}

guest_fixture! {
    pub fn optional_arguments_accept_null_and_undefined<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(Codec::module());
    |guest| {
        Codec::install(&guest);

        for source in [
            "codec.echo_optional()",
            "codec.echo_optional(undefined)",
            "codec.echo_optional(null)",
        ] {
            assert_eq!(guest.eval::<Option<String>>(source).unwrap(), None);
        }

        assert_eq!(
            guest.eval::<Option<String>>("codec.echo_optional('a')").unwrap(),
            Some("a".to_owned()),
        );

        for source in ["codec.echo_optional(1)", "codec.echo_required()"] {
            assert!(matches!(
                guest.eval::<Option<String>>(source),
                Err(Error::Guest(exception)) if exception.matches("TypeError"),
            ));
        }
    }
}

pub fn init_hooks_run_once_per_guest_in_bind_order<B>()
where
    B: BackendModules + BackendInterrupt,
{
    let log = Rc::new(RefCell::new(Vec::new()));
    let runtime = Runtime::<B>::builder()
        .bind(HostInitializer::new({
            let log = log.clone();

            move |_| {
                log.borrow_mut().push("first");

                Ok(())
            }
        }))
        .bind(HostInitializer::new({
            let log = log.clone();

            move |_| {
                log.borrow_mut().push("second");

                Ok(())
            }
        }))
        .build()
        .unwrap();
    let first = runtime.guest().build().unwrap();
    let second = runtime.guest().build().unwrap();

    assert_eq!(*log.borrow(), vec!["first", "second", "first", "second"],);

    drop((first, second));
}

#[macro_export]
macro_rules! __guestjs_backend_callables_tests {
    ($backend:ty) => {
        #[test]
        fn calls_a_host_function_both_ways() {
            $crate::fixtures::callables::calls_a_host_function_both_ways::<$backend>();
        }

        #[test]
        fn round_trips_integers() {
            $crate::fixtures::callables::round_trips_integers::<$backend>();
        }

        #[test]
        fn round_trips_floats_and_strings() {
            $crate::fixtures::callables::round_trips_floats_and_strings::<$backend>();
        }

        #[test]
        fn round_trips_sequences() {
            $crate::fixtures::callables::round_trips_sequences::<$backend>();
        }

        #[test]
        fn round_trips_mappings_and_optionals() {
            $crate::fixtures::callables::round_trips_mappings_and_optionals::<$backend>();
        }

        #[test]
        fn an_error_from_a_host_body_is_catchable() {
            $crate::fixtures::callables::an_error_from_a_host_body_is_catchable::<$backend>();
        }

        #[test]
        fn optional_arguments_accept_null_and_undefined() {
            $crate::fixtures::callables::optional_arguments_accept_null_and_undefined::<$backend>();
        }

        #[test]
        fn init_hooks_run_once_per_guest_in_bind_order() {
            $crate::fixtures::callables::init_hooks_run_once_per_guest_in_bind_order::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_callables_tests as tests;
