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
        any::TypeId,
        cell::Cell,
        marker::PhantomData,
        rc::Rc,
        task::{
            self,
            Poll,
        },
    };

    use crate::{
        backend::{
            callables::{
                BackendCallables,
                RawBody,
            },
            classes::BackendClasses,
            exceptions::BackendExceptions,
            interrupt::{
                BackendInterrupt,
                InterruptCondition,
            },
            library::BackendLibrary,
            modules::{
                BackendModules,
                ModuleDispatcher,
            },
            promises::{
                BackendPromises,
                PromiseState,
            },
            values::{
                BackendValues,
                Kind,
                Property,
                WellKnown,
            },
            Backend,
            Tok,
            Val,
        },
        errors::{
            Error,
            GuestErrorKind,
        },
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

    impl BackendValues for Stub {
        fn undefined<'js>(_token: Tok<'js, Self>) -> Val<'js, Self> {
            unimplemented!()
        }

        fn null<'js>(_token: Tok<'js, Self>) -> Val<'js, Self> {
            unimplemented!()
        }

        fn boolean<'js>(_token: Tok<'js, Self>, _value: bool) -> Val<'js, Self> {
            unimplemented!()
        }

        fn number<'js>(_token: Tok<'js, Self>, _value: f64) -> Val<'js, Self> {
            unimplemented!()
        }

        fn bigint<'js>(
            _token: Tok<'js, Self>,
            _value: i64,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn string<'js>(
            _token: Tok<'js, Self>,
            _value: &str,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn symbol<'js>(
            _token: Tok<'js, Self>,
            _description: &str,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn well_known<'js>(
            _token: Tok<'js, Self>,
            _symbol: WellKnown,
        ) -> Val<'js, Self> {
            unimplemented!()
        }

        fn array<'js>(
            _token: Tok<'js, Self>,
            _items: Vec<Val<'js, Self>>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn object<'js>(_token: Tok<'js, Self>) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn kind<'js>(_token: Tok<'js, Self>, _value: &Val<'js, Self>) -> Kind {
            unimplemented!()
        }

        fn as_bool<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Result<bool, Error> {
            unimplemented!()
        }

        fn as_f64<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Result<f64, Error> {
            unimplemented!()
        }

        fn as_i64<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Result<i64, Error> {
            unimplemented!()
        }

        fn as_string<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Result<String, Error> {
            unimplemented!()
        }

        fn display<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Result<String, Error> {
            unimplemented!()
        }

        fn same<'js>(
            _token: Tok<'js, Self>,
            _first: &Val<'js, Self>,
            _second: &Val<'js, Self>,
        ) -> bool {
            unimplemented!()
        }

        fn get<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _key: &Val<'js, Self>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn set<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _key: &Val<'js, Self>,
            _value: Val<'js, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }

        fn define<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _key: &Val<'js, Self>,
            _property: Property<'js, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }

        fn delete<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _key: &Val<'js, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }

        fn has<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _key: &Val<'js, Self>,
        ) -> Result<bool, Error> {
            unimplemented!()
        }

        fn keys<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
        ) -> Result<Vec<Val<'js, Self>>, Error> {
            unimplemented!()
        }

        fn instance_of<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
            _constructor: &Val<'js, Self>,
        ) -> Result<bool, Error> {
            unimplemented!()
        }

        fn prototype_of<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn set_prototype<'js>(
            _token: Tok<'js, Self>,
            _object: &Val<'js, Self>,
            _prototype: &Val<'js, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }

        fn call<'js>(
            _token: Tok<'js, Self>,
            _function: &Val<'js, Self>,
            _this: &Val<'js, Self>,
            _args: &[Val<'js, Self>],
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn construct<'js>(
            _token: Tok<'js, Self>,
            _constructor: &Val<'js, Self>,
            _args: &[Val<'js, Self>],
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendCallables for Stub {
        fn function<'js>(
            _token: Tok<'js, Self>,
            _name: &str,
            _length: u32,
            _body: RawBody<Self>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendClasses for Stub {
        type Ref<'a, C: 'static> = &'a C;
        type RefMut<'a, C: 'static> = &'a mut C;

        fn constructor<'js>(
            _token: Tok<'js, Self>,
            _name: &str,
            _length: u32,
            _body: RawBody<Self>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn instantiate<'js, C: 'static>(
            _token: Tok<'js, Self>,
            _prototype: &Val<'js, Self>,
            _payload: C,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn payload_type<'js>(
            _token: Tok<'js, Self>,
            _value: &Val<'js, Self>,
        ) -> Option<TypeId> {
            unimplemented!()
        }

        fn borrow<'js, 'a, C: 'static>(
            _token: Tok<'js, Self>,
            _value: &'a Val<'js, Self>,
        ) -> Result<Self::Ref<'a, C>, Error> {
            unimplemented!()
        }

        fn borrow_mut<'js, 'a, C: 'static>(
            _token: Tok<'js, Self>,
            _value: &'a Val<'js, Self>,
        ) -> Result<Self::RefMut<'a, C>, Error> {
            unimplemented!()
        }
    }

    impl BackendExceptions for Stub {
        fn throw<'js>(_token: Tok<'js, Self>, _value: Val<'js, Self>) -> Error {
            unimplemented!()
        }

        fn exception<'js>(
            _token: Tok<'js, Self>,
            _error: Error,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn error<'js>(
            _token: Tok<'js, Self>,
            _kind: GuestErrorKind,
            _message: &str,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendPromises for Stub {
        fn promise<'js>(
            _token: Tok<'js, Self>,
        ) -> Result<(Val<'js, Self>, Val<'js, Self>, Val<'js, Self>), Error> {
            unimplemented!()
        }

        fn resolve<'js>(
            _token: Tok<'js, Self>,
            _value: Val<'js, Self>,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn state<'js>(
            _token: Tok<'js, Self>,
            _promise: &Val<'js, Self>,
        ) -> Result<PromiseState<Val<'js, Self>>, Error> {
            unimplemented!()
        }

        fn poll_jobs(
            _engine: &Self::Engine,
            _context: &mut task::Context<'_>,
        ) -> Poll<Result<bool, Error>> {
            unimplemented!()
        }
    }

    impl BackendModules for Stub {
        type Module<'js> = ();

        fn eval<'js>(
            _token: Tok<'js, Self>,
            _source: &str,
            _name: &str,
        ) -> Result<Val<'js, Self>, Error> {
            unimplemented!()
        }

        fn declare<'js>(
            _token: Tok<'js, Self>,
            _name: &str,
            _source: &str,
        ) -> Result<Self::Module<'js>, Error> {
            unimplemented!()
        }

        fn synthetic<'js>(
            _token: Tok<'js, Self>,
            _name: &str,
            _exports: Vec<(String, Val<'js, Self>)>,
        ) -> Result<Self::Module<'js>, Error> {
            unimplemented!()
        }

        fn evaluate<'js>(
            _token: Tok<'js, Self>,
            _module: Self::Module<'js>,
        ) -> Result<(Val<'js, Self>, Val<'js, Self>), Error> {
            unimplemented!()
        }

        fn import<'js>(
            _token: Tok<'js, Self>,
            _name: &str,
        ) -> Result<(Val<'js, Self>, Val<'js, Self>), Error> {
            unimplemented!()
        }

        fn install(
            _engine: &Self::Engine,
            _dispatcher: Rc<dyn ModuleDispatcher<Self>>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
    }

    impl BackendLibrary for Stub {
        type NativeModule = ();

        fn declare_native<'js>(
            _token: Tok<'js, Self>,
            _native: &Self::NativeModule,
            _name: &str,
        ) -> Result<Self::Module<'js>, Error> {
            unimplemented!()
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
