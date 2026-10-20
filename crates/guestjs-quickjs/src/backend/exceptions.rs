use guestjs_core::{
    backend::{Tok, Val, exceptions::BackendExceptions},
    errors::{Error, GuestErrorKind},
};
use rquickjs::Object;

use crate::{
    backend::QuickJs,
    error::{Capture, Create, Raise, Thrown},
};

impl BackendExceptions for QuickJs {
    fn throw<'js>(token: Tok<'js, Self>, value: Val<'js, Self>) -> Error {
        Thrown(value).into_error(token)
    }

    fn exception<'js>(token: Tok<'js, Self>, error: Error) -> Result<Val<'js, Self>, Error> {
        error.exception(token).capture(token)
    }

    fn error<'js>(
        token: Tok<'js, Self>,
        kind: GuestErrorKind,
        message: &str,
    ) -> Result<Val<'js, Self>, Error> {
        kind.create(token, message)
            .map(Object::into_value)
            .capture(token)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::{
        backend::{Backend, exceptions::BackendExceptions, values::BackendValues},
        errors::{Error, GuestErrorKind},
    };
    use rquickjs::Value;

    use crate::{backend::QuickJs, engine::Config};

    guestjs_core::fixtures::exceptions::tests!(QuickJs);

    #[test]
    fn exception_returns_the_value_a_guest_catches() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let thrown = QuickJs::error(token, GuestErrorKind::RangeError, "out").unwrap();

            assert!(QuickJs::same(
                token,
                &QuickJs::exception(token, QuickJs::throw(token, thrown.clone())).unwrap(),
                &thrown,
            ));
            assert_eq!(
                QuickJs::as_string(
                    token,
                    &QuickJs::get(
                        token,
                        &QuickJs::exception(token, Error::conversion("i64", "string")).unwrap(),
                        &QuickJs::new_string(token, "name").unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap(),
                "TypeError",
            );
        });
    }

    #[test]
    fn error_uses_intrinsic_constructors() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let intrinsic = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    const intrinsic = TypeError;
                    globalThis.TypeError = function () {};
                    intrinsic
                    "#,
                )
                .unwrap();
            let message = format!("{}\0end", "a".repeat(300));
            let error = QuickJs::error(token, GuestErrorKind::TypeError, &message).unwrap();

            assert!(QuickJs::instance_of(token, &error, &intrinsic).unwrap());
            assert_eq!(
                QuickJs::as_string(
                    token,
                    &QuickJs::get(token, &error, &QuickJs::new_string(token, "message").unwrap(),)
                        .unwrap(),
                )
                .unwrap(),
                message,
            );
        });
    }

    #[test]
    fn throw_captures_without_raising() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let error = QuickJs::error(token, GuestErrorKind::RangeError, "out").unwrap();

            let Error::Guest(exception) = QuickJs::throw(token, error.clone()) else {
                panic!("expected a guest exception");
            };

            assert_eq!(exception.name(), "RangeError");
            assert!(exception.matches("RangeError"));
            assert!(!token.ctx().has_exception());
            assert!(QuickJs::same(
                token,
                &QuickJs::attach(token, exception.value().unwrap()).unwrap(),
                &error,
            ));
        });
    }
}
