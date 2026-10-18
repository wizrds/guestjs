pub mod callables;
pub mod classes;
pub mod exceptions;
pub mod interrupt;
pub mod library;
pub mod modules;
pub mod promises;
pub mod values;

use std::{
    fmt::Debug,
    hash::Hash,
};

use crate::errors::Error;

pub type Val<'js, B> = <B as Backend>::Value<'js>;
pub type Tok<'js, B> = <B as Backend>::Token<'js>;

pub trait Backend: Sized + 'static {
    type Engine;
    type Context;
    type Token<'js>: Copy;
    type Value<'js>: Clone + Debug;
    type Owned: Clone + 'static;
    type Config: Default;
    type ContextId: Copy + Eq + Hash + Debug + 'static;

    const NAME: &'static str;

    fn engine(config: Self::Config) -> Result<Self::Engine, Error>;

    fn new_context(engine: &Self::Engine) -> Result<Self::Context, Error>;

    fn enter<F, R>(
        engine: &Self::Engine,
        context: &Self::Context,
        f: F,
    ) -> R
    where
        F: for<'js> FnOnce(Tok<'js, Self>) -> R;

    fn context_id<'js>(token: Tok<'js, Self>) -> Self::ContextId;

    fn globals<'js>(token: Tok<'js, Self>) -> Val<'js, Self>;

    fn detach<'js>(
        token: Tok<'js, Self>,
        value: Val<'js, Self>,
    ) -> Self::Owned;

    fn attach<'js>(
        token: Tok<'js, Self>,
        owned: &Self::Owned,
    ) -> Result<Val<'js, Self>, Error>;

    fn owned_ptr_eq(first: &Self::Owned, second: &Self::Owned) -> bool;
}

#[cfg(test)]
pub mod tests {
    use std::{
        cell::Cell,
        marker::PhantomData,
    };

    use crate::{
        backend::{
            interrupt::{
                BackendInterrupt,
                InterruptCondition,
            },
            Backend,
            Tok,
            Val,
        },
        errors::Error,
    };

    pub struct Stub;

    #[derive(Clone, Copy)]
    pub struct StubToken<'js> {
        pub context: u64,
        pub brand: PhantomData<&'js ()>,
    }

    impl Backend for Stub {
        type Engine = Cell<u64>;
        type Context = u64;
        type Token<'js> = StubToken<'js>;
        type Value<'js> = ();
        type Owned = ();
        type Config = ();
        type ContextId = u64;

        const NAME: &'static str = "stub";

        fn engine(_config: Self::Config) -> Result<Self::Engine, Error> {
            Ok(Cell::default())
        }

        fn new_context(engine: &Self::Engine) -> Result<Self::Context, Error> {
            engine.set(engine.get() + 1);

            Ok(engine.get())
        }

        fn enter<F, R>(
            _engine: &Self::Engine,
            context: &Self::Context,
            f: F,
        ) -> R
        where
            F: for<'js> FnOnce(Tok<'js, Self>) -> R,
        {
            f(StubToken {
                context: *context,
                brand: PhantomData,
            })
        }

        fn context_id<'js>(token: Tok<'js, Self>) -> Self::ContextId {
            token.context
        }

        fn globals<'js>(_token: Tok<'js, Self>) -> Val<'js, Self> {}

        fn detach<'js>(
            _token: Tok<'js, Self>,
            _value: Val<'js, Self>,
        ) -> Self::Owned {}

        fn attach<'js>(
            _token: Tok<'js, Self>,
            _owned: &Self::Owned,
        ) -> Result<Val<'js, Self>, Error> {
            Ok(())
        }

        fn owned_ptr_eq(_first: &Self::Owned, _second: &Self::Owned) -> bool {
            true
        }
    }

    impl BackendInterrupt for Stub {
        fn set_interrupt_condition(
            _engine: &Self::Engine,
            _condition: InterruptCondition,
        ) {}

        fn check<'js>(_token: Tok<'js, Self>) -> Result<(), Error> {
            Ok(())
        }
    }
}
