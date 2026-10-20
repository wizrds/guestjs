use std::collections::HashMap;

use crate::{
    backend::{callables::BackendCallables, interrupt::BackendInterrupt, modules::BackendModules},
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::Error,
    eval::{GuestEvalExt, GuestGlobalsExt},
    fixtures::guest_fixture,
    handle::protocol::{CallableProtocol, ObjectProtocol},
    host::{function::FunctionDeclaration, module::ModuleDeclaration},
    marshal::{FromGuest, ToGuest},
    runtime::Runtime,
};

guest_fixture! {
    pub fn runs_guest_javascript<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        function double(value) {
            return value * 2;
        }
        "#,
            )
            .unwrap();

        assert_eq!(guest.eval::<f64>("double(21)").unwrap(), 42.0);
    }
}

guest_fixture! {
    pub fn calls_a_host_function_passed_as_an_argument<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        function invoke(options) {
            return options.callback(21);
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .globals()
                .unwrap()
                .function("invoke")
                .unwrap()
                .call::<_, i64>((HashMap::from([(
                    "callback",
                    FunctionDeclaration::<B>::new(|context, args| {
                        Ok::<_, Error>(args.next::<i64>(context.enter())? * 2)
                    }),
                )]),))
                .unwrap(),
            42,
        );
    }
}

guest_fixture! {
    pub fn runs_with_only_synchronous_capability_bounds<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendCallables,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("sync_host")
            .value("answer", 42i64)
            .function("double", |context, args| {
                Ok::<_, Error>(args.next::<i64>(context.enter())? * 2)
            }),
    );
    |guest| {
        assert_eq!(
            guest
                .enter(|enter| {
                    let (namespace, _) = B::import(enter.token(), "sync_host")?;

                    i64::from_guest(
                        enter,
                        B::call(
                            enter.token(),
                            &B::get(
                                enter.token(),
                                &namespace,
                                &"double".to_guest(enter)?,
                            )?,
                            &B::new_undefined(enter.token()),
                            &[B::get(
                                enter.token(),
                                &namespace,
                                &"answer".to_guest(enter)?,
                            )?],
                        )?,
                    )
                })
                .unwrap(),
            84,
        );
    }
}

#[macro_export]
macro_rules! __guestjs_backend_tests {
    ($backend:ty) => {
        #[test]
        fn runs_guest_javascript() {
            $crate::fixtures::backend::runs_guest_javascript::<$backend>();
        }

        #[test]
        fn calls_a_host_function_passed_as_an_argument() {
            $crate::fixtures::backend::calls_a_host_function_passed_as_an_argument::<$backend>();
        }

        #[test]
        fn runs_with_only_synchronous_capability_bounds() {
            $crate::fixtures::backend::runs_with_only_synchronous_capability_bounds::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_tests as tests;
