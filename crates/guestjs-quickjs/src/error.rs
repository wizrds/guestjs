use guestjs_core::errors::{
    Error,
    GuestErrorKind,
    GuestException,
};
use rquickjs::{
    atom::PredefinedAtom,
    object::Property,
    qjs,
    Coerced,
    Object,
    Symbol,
    Type,
    Value,
};

use crate::{
    context::Token,
    value::Owned,
};

pub trait Capture<T> {
    fn convert(self) -> Result<T, Error>;

    fn capture(self, token: Token<'_>) -> Result<T, Error>;
}

impl<T> Capture<T> for rquickjs::Result<T> {
    fn convert(self) -> Result<T, Error> {
        self.map_err(|error| match error {
            rquickjs::Error::FromJs { from, to, .. } => {
                Error::conversion(to, from)
            }
            error => Error::engine(error.to_string()),
        })
    }

    fn capture(self, token: Token<'_>) -> Result<T, Error> {
        match self {
            Err(rquickjs::Error::Exception) => Err(Thrown(token.ctx().catch()).into_error(token)),
            result => result.convert(),
        }
    }
}

pub struct Thrown<'js>(pub Value<'js>);

impl<'js> Thrown<'js> {
    pub fn into_error(self, token: Token<'js>) -> Error {
        self.exception(token)
            .map(Error::guest)
            .unwrap_or_else(|error| error)
    }

    pub fn exception(self, token: Token<'js>) -> Result<GuestException, Error> {
        if unsafe { qjs::JS_IsUncatchableError(self.0.as_raw()) } {
            return Err(Error::interrupted());
        }

        let error = self
            .0
            .as_object()
            .filter(|object| object.is_error());

        let name = error
            .map(|error| Self::settle(token, error.get::<_, Coerced<String>>(PredefinedAtom::Name)))
            .transpose()?
            .flatten()
            .map_or_else(|| self.type_of().to_string(), |name| name.0);

        let message = match error {
            Some(error) => Self::settle(
                token,
                error
                    .get::<_, Coerced<String>>(PredefinedAtom::Message)
                    .map(|message| message.0),
            )?,
            None => Self::settle(
                token,
                match Symbol::from_value(self.0.clone()) {
                    Ok(symbol) => symbol
                        .description()
                        .and_then(|description| description.get::<Option<String>>())
                        .map(|description| format!("Symbol({})", description.unwrap_or_default())),
                    Err(_) => self
                        .0
                        .get::<Coerced<String>>()
                        .map(|string| string.0),
                },
            )?,
        }
        .unwrap_or_default();

        let stack = error
            .map(|error| Self::settle(token, error.get::<_, Value>(PredefinedAtom::Stack)))
            .transpose()?
            .flatten()
            .and_then(|stack| {
                stack
                    .as_string()
                    .and_then(|stack| stack.to_string().ok())
            });

        let mut prototypes = Vec::new();
        let mut current = self.0.as_object().cloned();

        while let Some(object) = current {
            current = Self::settle(
                token,
                match unsafe { qjs::JS_GetPrototype(token.pointer().as_ptr(), object.as_raw()) } {
                    raw if unsafe { qjs::JS_IsException(raw) } => Err(rquickjs::Error::Exception),
                    raw => Ok(unsafe { Value::from_raw(token.ctx(), raw) }.into_object()),
                },
            )?
            .flatten();

            if let Some(name) = current
                .as_ref()
                .map(|prototype| {
                    Self::settle(
                        token,
                        prototype
                            .get::<_, Object>(PredefinedAtom::Constructor)
                            .and_then(|constructor| constructor.get::<_, String>(PredefinedAtom::Name)),
                    )
                })
                .transpose()?
                .flatten()
            {
                prototypes.push(name);
            }
        }

        Ok(GuestException::new(
            name,
            message,
            stack,
            prototypes,
            Owned::new(token, self.0),
        ))
    }

    pub fn type_of(&self) -> &'static str {
        match self.0.type_of() {
            Type::Uninitialized | Type::Undefined => "undefined",
            Type::Bool => "boolean",
            Type::Int | Type::Float => "number",
            Type::BigInt => "bigint",
            Type::String => "string",
            Type::Symbol => "symbol",
            Type::Constructor | Type::Function => "function",
            _ => "object",
        }
    }

    pub fn settle<V>(
        token: Token<'js>,
        result: rquickjs::Result<V>,
    ) -> Result<Option<V>, Error> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(rquickjs::Error::Exception) => {
                match unsafe { qjs::JS_IsUncatchableError(token.ctx().catch().as_raw()) } {
                    true => Err(Error::interrupted()),
                    false => Ok(None),
                }
            }
            Err(_) => Ok(None),
        }
    }
}

pub trait Create {
    fn create<'js>(
        self,
        token: Token<'js>,
        message: &str,
    ) -> rquickjs::Result<Object<'js>>;

    fn throw(
        self,
        token: Token<'_>,
        message: &str,
    ) -> rquickjs::Error;
}

impl Create for GuestErrorKind {
    fn create<'js>(
        self,
        token: Token<'js>,
        message: &str,
    ) -> rquickjs::Result<Object<'js>> {
        match unsafe {
            match self {
                GuestErrorKind::Error => qjs::JS_NewPlainError(token.pointer().as_ptr(), c"".as_ptr()),
                GuestErrorKind::TypeError => qjs::JS_NewTypeError(token.pointer().as_ptr(), c"".as_ptr()),
                GuestErrorKind::RangeError => qjs::JS_NewRangeError(token.pointer().as_ptr(), c"".as_ptr()),
                GuestErrorKind::ReferenceError => {
                    qjs::JS_NewReferenceError(token.pointer().as_ptr(), c"".as_ptr())
                }
                GuestErrorKind::SyntaxError => qjs::JS_NewSyntaxError(token.pointer().as_ptr(), c"".as_ptr()),
            }
        } {
            raw if unsafe { qjs::JS_IsException(raw) } => Err(rquickjs::Error::Exception),
            raw => Object::from_value(unsafe { Value::from_raw(token.ctx(), raw) }).and_then(|error| {
                error.prop(
                    PredefinedAtom::Message,
                    Property::from(message)
                        .writable()
                        .configurable(),
                )?;

                Ok(error)
            }),
        }
    }

    fn throw(
        self,
        token: Token<'_>,
        message: &str,
    ) -> rquickjs::Error {
        self.create(token, message)
            .map(|error| token.ctx().throw(error.into_value()))
            .unwrap_or_else(|error| error)
    }
}

pub trait Raise {
    fn raise(self, token: Token<'_>) -> rquickjs::Error;
}

impl Raise for Error {
    fn raise(self, token: Token<'_>) -> rquickjs::Error {
        match self {
            Error::Guest(exception) => match exception
                .value::<Owned>()
                .map(|owned| owned.restore(token))
            {
                Some(Ok(value)) => token.ctx().throw(value),
                _ => GuestErrorKind::Error
                    .create(token, exception.message())
                    .and_then(|error| {
                        error.prop(
                            PredefinedAtom::Name,
                            Property::from(exception.name())
                                .writable()
                                .configurable(),
                        )?;

                        if let Some(stack) = exception.stack() {
                            error.prop(
                                PredefinedAtom::Stack,
                                Property::from(stack)
                                    .writable()
                                    .configurable(),
                            )?;
                        }

                        Ok(token.ctx().throw(error.into_value()))
                    })
                    .unwrap_or_else(|error| error),
            },
            error => error
                .guest_kind()
                .create(token, &error.to_string())
                .map(|object| {
                    if error.is_interrupted() {
                        unsafe { qjs::JS_SetUncatchableError(token.pointer().as_ptr(), object.as_raw()) };
                    }

                    token.ctx().throw(object.into_value())
                })
                .unwrap_or_else(|error| error),
        }
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::errors::Error;
    use rquickjs::{
        qjs,
        Object,
        Value,
    };

    use crate::{
        context::Context,
        engine::{
            Config,
            Engine,
        },
        error::{
            Capture,
            Raise,
        },
        value::Owned,
    };

    #[test]
    fn capture_keeps_the_thrown_value() {
        let engine = Engine::new(Config::default()).unwrap();
        let context = Context::new(&engine).unwrap();

        engine.enter(&context, |token| {
            let Err(Error::Guest(exception)) = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    class Retryable extends Error {}
                    globalThis.thrown = new Retryable("again");
                    throw thrown;
                    "#,
                )
                .capture(token)
            else {
                panic!("expected a guest exception");
            };

            assert_eq!(exception.name(), "Error");
            assert_eq!(exception.message(), "again");
            assert!(exception.matches("Retryable"));
            assert!(exception.stack().is_some());
            assert!(!token
                .ctx()
                .has_exception());
            assert_eq!(
                exception
                    .value::<Owned>()
                    .unwrap()
                    .restore(token)
                    .unwrap(),
                token
                    .ctx()
                    .globals()
                    .get::<_, Value>("thrown")
                    .unwrap(),
            );
        });
    }

    #[test]
    fn capture_describes_non_errors() {
        let engine = Engine::new(Config::default()).unwrap();
        let context = Context::new(&engine).unwrap();

        engine.enter(&context, |token| {
            let capture = |source: &str| match token
                .ctx()
                .eval::<Value, _>(source)
                .capture(token)
            {
                Err(Error::Guest(exception)) => exception,
                _ => panic!("expected a guest exception"),
            };

            let string = capture("throw 'x'");

            assert_eq!(string.name(), "string");
            assert_eq!(string.message(), "x");
            assert_eq!(string.stack(), None);
            assert!(!string.matches("String"));
            assert_eq!(capture("throw Symbol('s')").message(), "Symbol(s)");
        });
    }

    #[test]
    fn capture_survives_throwing_getters() {
        let engine = Engine::new(Config::default()).unwrap();
        let context = Context::new(&engine).unwrap();

        engine.enter(&context, |token| {
            let Err(Error::Guest(exception)) = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    const error = new Error("x");
                    Object.defineProperty(error, "name", { get() { throw new Error("secondary"); } });
                    throw error;
                    "#,
                )
                .capture(token)
            else {
                panic!("expected a guest exception");
            };

            assert_eq!(exception.name(), "object");
            assert_eq!(exception.message(), "x");
            assert!(!token
                .ctx()
                .has_exception());
        });
    }

    #[test]
    fn raise_rethrows_the_same_value() {
        let engine = Engine::new(Config::default()).unwrap();
        let context = Context::new(&engine).unwrap();

        engine.enter(&context, |token| {
            token
                .ctx()
                .eval::<Value, _>("throw (globalThis.thrown = new Error('same'))")
                .capture(token)
                .unwrap_err()
                .raise(token);

            assert_eq!(
                token
                    .ctx()
                    .catch(),
                token
                    .ctx()
                    .globals()
                    .get::<_, Value>("thrown")
                    .unwrap(),
            );
        });
    }

    #[test]
    fn raise_copies_foreign_values() {
        let engine = Engine::new(Config::default()).unwrap();
        let first = Context::new(&engine).unwrap();
        let second = Context::new(&engine).unwrap();

        let error = engine.enter(&first, |token| {
            token
                .ctx()
                .eval::<Value, _>("throw new RangeError('far')")
                .capture(token)
                .unwrap_err()
        });

        engine.enter(&second, |token| {
            error.raise(token);

            let caught = Object::from_value(
                token
                    .ctx()
                    .catch(),
            )
            .unwrap();

            assert_eq!(caught.get::<_, String>("name").unwrap(), "RangeError");
            assert_eq!(caught.get::<_, String>("message").unwrap(), "far");
        });
    }

    #[test]
    fn raise_maps_host_errors() {
        let engine = Engine::new(Config::default()).unwrap();
        let context = Context::new(&engine).unwrap();

        engine.enter(&context, |token| {
            Error::conversion("i64", "string").raise(token);

            assert_eq!(
                Object::from_value(
                    token
                        .ctx()
                        .catch(),
                )
                .unwrap()
                .get::<_, String>("name")
                .unwrap(),
                "TypeError",
            );

            Error::interrupted().raise(token);

            assert!(unsafe {
                qjs::JS_IsUncatchableError(
                    token
                        .ctx()
                        .catch()
                        .as_raw(),
                )
            });
        });
    }
}
