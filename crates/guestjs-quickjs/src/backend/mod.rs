pub mod buffers;
pub mod callables;
pub mod classes;
pub mod exceptions;
pub mod interrupt;
pub mod library;
pub mod modules;
pub mod promises;
pub mod values;

use std::ptr::NonNull;

use guestjs_core::{
    backend::{
        Backend,
        Tok,
        Val,
    },
    errors::Error,
};
use rquickjs::{
    qjs::JSContext,
    Value,
};

use crate::{
    context::{
        Context,
        Token,
    },
    engine::{
        Config,
        Engine,
    },
    value::Owned,
};

pub struct QuickJs;

impl Backend for QuickJs {
    type Engine = Engine;
    type Context = Context;
    type Token<'js> = Token<'js>;
    type Value<'js> = Value<'js>;
    type Owned = Owned;
    type Config = Config;
    type ContextId = NonNull<JSContext>;

    const NAME: &'static str = "quickjs";

    fn engine(config: Self::Config) -> Result<Self::Engine, Error> {
        Engine::new(config)
    }

    fn new_context(engine: &Self::Engine) -> Result<Self::Context, Error> {
        Context::new(engine)
    }

    fn enter<F, R>(
        engine: &Self::Engine,
        context: &Self::Context,
        f: F,
    ) -> R
    where
        F: for<'js> FnOnce(Tok<'js, Self>) -> R,
    {
        engine.enter(context, f)
    }

    fn context_id<'js>(token: Tok<'js, Self>) -> Self::ContextId {
        token.pointer()
    }

    fn globals<'js>(token: Tok<'js, Self>) -> Val<'js, Self> {
        token
            .ctx()
            .globals()
            .into_value()
    }

    fn detach<'js>(
        token: Tok<'js, Self>,
        value: Val<'js, Self>,
    ) -> Self::Owned {
        Owned::new(token, value)
    }

    fn attach<'js>(
        token: Tok<'js, Self>,
        owned: &Self::Owned,
    ) -> Result<Val<'js, Self>, Error> {
        owned.restore(token)
    }

    fn owned_ptr_eq(first: &Self::Owned, second: &Self::Owned) -> bool {
        first.is_same(second)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::backend::Backend;
    use rquickjs::{
        Object,
        Value,
    };

    use crate::{
        backend::QuickJs,
        engine::Config,
    };

    #[test]
    fn nested_enter_reaches_another_context() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let first = QuickJs::new_context(&engine).unwrap();
        let second = QuickJs::new_context(&engine).unwrap();

        assert!(QuickJs::enter(&engine, &first, |outer| {
            QuickJs::enter(&engine, &second, |inner| inner.pointer() != outer.pointer())
        }));
    }

    #[test]
    fn attach_in_another_context_is_foreign() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let first = QuickJs::new_context(&engine).unwrap();
        let second = QuickJs::new_context(&engine).unwrap();

        let owned = QuickJs::enter(&engine, &first, |token| {
            QuickJs::detach(token, Value::new_number(token.ctx(), 1.0))
        });

        assert!(
            QuickJs::enter(&engine, &second, |token| {
                QuickJs::attach(token, &owned).map(drop)
            })
            .is_err_and(|error| error.is_foreign_context()),
        );
    }

    #[test]
    fn context_dropped_inside_entry_is_freed() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let first = QuickJs::new_context(&engine).unwrap();
        let second = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &first, |_| drop(second));
    }

    #[test]
    fn new_context_inside_entry_is_unsupported() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        assert!(
            QuickJs::enter(&engine, &context, |_| QuickJs::new_context(&engine))
                .is_err_and(|error| error.is_unsupported()),
        );
    }

    #[test]
    fn owned_outlives_engine_and_context() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        let owned = QuickJs::enter(&engine, &context, |token| {
            QuickJs::detach(token, Value::new_number(token.ctx(), 1.0))
        });

        drop(context);
        drop(engine);
        drop(owned);
    }

    #[test]
    fn owned_ptr_eq_follows_identity() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        let (first, second, other) = QuickJs::enter(&engine, &context, |token| {
            (
                QuickJs::detach(token, QuickJs::globals(token)),
                QuickJs::detach(token, QuickJs::globals(token)),
                QuickJs::detach(
                    token,
                    Object::new(token.ctx())
                        .unwrap()
                        .into_value(),
                ),
            )
        });

        assert!(QuickJs::owned_ptr_eq(&first, &second));
        assert!(!QuickJs::owned_ptr_eq(&first, &other));
    }

    #[test]
    fn context_id_names_the_entered_context() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let first = QuickJs::new_context(&engine).unwrap();
        let second = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &first, |outer| {
            assert_eq!(
                QuickJs::enter(&engine, &first, QuickJs::context_id),
                QuickJs::context_id(outer),
            );
            assert_ne!(
                QuickJs::enter(&engine, &second, QuickJs::context_id),
                QuickJs::context_id(outer),
            );
        });
    }
}
