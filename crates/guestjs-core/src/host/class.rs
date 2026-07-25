use std::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
};

use rquickjs::{
    Array, Atom, CatchResultExt, Class, Constructor, Ctx, Exception, FromJs,
    Function as JsFunction, JsLifetime, Object as JsObject, Symbol, Value,
    class::{JsClass, OwnedBorrow, OwnedBorrowMut, Trace, Tracer, Writable},
    function::{Async, Rest, This},
    object::Accessor,
};

use crate::{
    errors::Error,
    host::{args::Args, namespace::Namespace},
    marshal::{
        FromGuest, FromGuestBound, FromGuestMut, FromGuestRef, ToGuest, ToGuestBound,
    },
    runtime::Scope,
};

/// A Rust class exposed to guest code.
pub trait HostClass: Sized + 'static {
    /// The guest-visible class name.
    const NAME: &'static str;

    /// Constructs an instance.
    fn construct<'js>(scope: &Scope<'js>, args: Args<'js>) -> Result<Self, Error>;

    /// Defines the class.
    fn build(spec: &mut ClassSpec<Self>);
}

/// A JavaScript well-known symbol.
#[derive(Clone, Copy)]
pub enum WellKnownSymbol {
    /// `Symbol.iterator`.
    Iterator,
    /// `Symbol.asyncIterator`.
    AsyncIterator,
    /// `Symbol.toPrimitive`.
    ToPrimitive,
    /// `Symbol.hasInstance`.
    HasInstance,
}

impl WellKnownSymbol {
    fn atom<'js>(self, ctx: &Ctx<'js>) -> Atom<'js> {
        match self {
            Self::Iterator => Symbol::iterator(ctx.clone()).as_atom(),
            Self::AsyncIterator => Symbol::async_iterator(ctx.clone()).as_atom(),
            Self::ToPrimitive => Symbol::to_primitive(ctx.clone()).as_atom(),
            Self::HasInstance => Symbol::has_instance(ctx.clone()).as_atom(),
        }
    }
}

type MethodResult<'js> = Result<Value<'js>, Error>;
type MethodFuture<'js> = Pin<Box<dyn Future<Output = MethodResult<'js>> + 'js>>;
type GetterSpec<C> = Box<dyn for<'js> Fn(&C, &Scope<'js>) -> MethodResult<'js>>;
type SetterSpec<C> = Box<dyn for<'js> Fn(&mut C, &Scope<'js>, Value<'js>) -> Result<(), Error>>;
type RefMethod<C> = Box<
    dyn for<'js> Fn(&C, &Scope<'js>, Args<'js>) -> MethodResult<'js>,
>;
type MutMethod<C> = Box<
    dyn for<'js> Fn(&mut C, &Scope<'js>, Args<'js>) -> MethodResult<'js>,
>;
type AsyncRefMethod<C> = Box<
    dyn for<'js> Fn(
        &C,
        &Scope<'js>,
        Args<'js>,
    ) -> Result<MethodFuture<'js>, Error>,
>;
type AsyncMutMethod<C> = Box<
    dyn for<'js> Fn(
        &mut C,
        &Scope<'js>,
        Args<'js>,
    ) -> Result<MethodFuture<'js>, Error>,
>;
type AccessorSpec<C> = (
    String,
    Option<GetterSpec<C>>,
    Option<SetterSpec<C>>,
);

enum MethodSpec<C> {
    Ref(RefMethod<C>),
    Mut(MutMethod<C>),
    AsyncRef(AsyncRefMethod<C>),
    AsyncMut(AsyncMutMethod<C>),
}

/// A host class definition.
pub struct ClassSpec<C> {
    methods: Vec<(String, MethodSpec<C>)>,
    accessors: Vec<AccessorSpec<C>>,
    symbols: Vec<(WellKnownSymbol, MethodSpec<C>)>,
    statics: Namespace,
}

struct ClassParts<C> {
    methods: Vec<(String, MethodSpec<C>)>,
    accessors: Vec<AccessorSpec<C>>,
    symbols: Vec<(WellKnownSymbol, MethodSpec<C>)>,
    statics: Namespace,
}

impl<C: HostClass> ClassSpec<C> {
    pub(crate) fn new() -> Self {
        Self {
            methods: Vec::new(),
            accessors: Vec::new(),
            symbols: Vec::new(),
            statics: Namespace::new(),
        }
    }

    /// Defines a shared method.
    pub fn method<F, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&C, &Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.methods.push((
            name.to_owned(),
            MethodSpec::Ref(Box::new(move |class, scope, args| {
                f(class, scope, args)?.to_guest(scope)
            })),
        ));

        self
    }

    /// Defines an exclusive method.
    pub fn method_mut<F, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&mut C, &Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.methods.push((
            name.to_owned(),
            MethodSpec::Mut(Box::new(move |class, scope, args| {
                f(class, scope, args)?.to_guest(scope)
            })),
        ));

        self
    }

    /// Defines an asynchronous shared method.
    pub fn async_method<F, Fut, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&C, &Scope<'js>, Args<'js>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest,
    {
        self.methods.push((
            name.to_owned(),
            MethodSpec::AsyncRef(Box::new(move |class, scope, args| {
                let future = f(class, scope, args)?;
                let scope = scope.clone();

                Ok(Box::pin(async move { future.await?.to_guest(&scope) }))
            })),
        ));

        self
    }

    /// Defines an asynchronous exclusive method.
    pub fn async_method_mut<F, Fut, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&mut C, &Scope<'js>, Args<'js>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest,
    {
        self.methods.push((
            name.to_owned(),
            MethodSpec::AsyncMut(Box::new(move |class, scope, args| {
                let future = f(class, scope, args)?;
                let scope = scope.clone();

                Ok(Box::pin(async move { future.await?.to_guest(&scope) }))
            })),
        ));

        self
    }

    /// Defines a getter.
    pub fn getter<F, R>(&mut self, name: &str, get: F) -> &mut Self
    where
        F: for<'js> Fn(&C, &Scope<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.accessors.push((
            name.to_owned(),
            Some(Box::new(move |class, scope| get(class, scope)?.to_guest(scope))),
            None,
        ));

        self
    }

    /// Defines a setter.
    pub fn setter<F, V>(&mut self, name: &str, set: F) -> &mut Self
    where
        F: for<'js> Fn(&mut C, &Scope<'js>, V::Bound<'js>) -> Result<(), Error> + 'static,
        V: FromGuestBound,
    {
        self.accessors.push((
            name.to_owned(),
            None,
            Some(Box::new(move |class, scope, value| {
                set(class, scope, V::from_guest_bound(scope, value)?)
            })),
        ));

        self
    }

    /// Defines a getter and setter.
    pub fn accessor<G, S, R, V>(&mut self, name: &str, get: G, set: S) -> &mut Self
    where
        G: for<'js> Fn(&C, &Scope<'js>) -> Result<R, Error> + 'static,
        S: for<'js> Fn(&mut C, &Scope<'js>, V::Bound<'js>) -> Result<(), Error> + 'static,
        R: ToGuest,
        V: FromGuestBound,
    {
        self.accessors.push((
            name.to_owned(),
            Some(Box::new(move |class, scope| get(class, scope)?.to_guest(scope))),
            Some(Box::new(move |class, scope, value| {
                set(class, scope, V::from_guest_bound(scope, value)?)
            })),
        ));

        self
    }

    /// Defines a well-known symbol method.
    pub fn symbol_method<F, R>(&mut self, symbol: WellKnownSymbol, f: F) -> &mut Self
    where
        F: for<'js> Fn(&C, &Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.symbols.push((
            symbol,
            MethodSpec::Ref(Box::new(move |class, scope, args| {
                f(class, scope, args)?.to_guest(scope)
            })),
        ));

        self
    }

    /// Defines `Symbol.iterator`.
    pub fn iterable<F, I, T>(&mut self, make: F) -> &mut Self
    where
        F: for<'js> Fn(&C, &Scope<'js>) -> Result<I, Error> + 'static,
        I: IntoIterator<Item = T>,
        T: ToGuest,
    {
        self.symbols.push((
            WellKnownSymbol::Iterator,
            MethodSpec::Ref(Box::new(move |class, scope, _args| {
                let array = Array::new(scope.ctx().clone()).catch(scope.ctx())?;

                for (index, item) in make(class, scope)?
                    .into_iter()
                    .enumerate()
                {
                    array
                        .set(index, item.to_guest(scope)?)
                        .catch(scope.ctx())?;
                }

                array
                    .as_object()
                    .get::<_, JsFunction>(Symbol::iterator(scope.ctx().clone()).as_atom())
                    .catch(scope.ctx())?
                    .call::<_, Value>((This(array.clone()),))
                    .catch(scope.ctx())
                    .map_err(Into::into)
            })),
        ));

        self
    }

    /// Defines static members.
    pub fn statics<F>(&mut self, build: F) -> &mut Self
    where
        F: FnOnce(&mut Namespace),
    {
        build(&mut self.statics);

        self
    }

    fn into_parts(self) -> ClassParts<C> {
        ClassParts {
            methods: self.methods,
            accessors: self.accessors,
            symbols: self.symbols,
            statics: self.statics,
        }
    }
}

impl<C: HostClass> MethodSpec<C> {
    fn into_function<'js>(self, ctx: &Ctx<'js>) -> rquickjs::Result<JsFunction<'js>> {
        match self {
            Self::Ref(thunk) => JsFunction::new(
                ctx.clone(),
                move |this: This<Class<'js, HostInstance<C>>>,
                      args: Rest<Value<'js>>|
                      -> rquickjs::Result<Value<'js>> {
                    let scope = Scope::detached(this.0.ctx().clone());
                    let guard = this.0.try_borrow()?;

                    thunk(&(*guard).0, &scope, Args::new(args.0))
                        .map_err(|error| Exception::throw_message(scope.ctx(), &error.to_string()))
                },
            ),
            Self::Mut(thunk) => JsFunction::new(
                ctx.clone(),
                move |this: This<Class<'js, HostInstance<C>>>,
                      args: Rest<Value<'js>>|
                      -> rquickjs::Result<Value<'js>> {
                    let scope = Scope::detached(this.0.ctx().clone());
                    let mut guard = this.0.try_borrow_mut()?;

                    thunk(&mut (*guard).0, &scope, Args::new(args.0))
                        .map_err(|error| Exception::throw_message(scope.ctx(), &error.to_string()))
                },
            ),
            Self::AsyncRef(thunk) => JsFunction::new(
                ctx.clone(),
                Async(move |this: This<Class<'js, HostInstance<C>>>, args: Rest<Value<'js>>| {
                    let ctx = this.0.ctx().clone();

                    let future = this
                        .0
                        .try_borrow()
                        .map_err(Into::into)
                        .and_then(|guard| {
                            thunk(&(*guard).0, &Scope::detached(ctx.clone()), Args::new(args.0))
                        });

                    async move {
                        match future {
                            Ok(future) => future.await,
                            Err(error) => Err(error),
                        }
                        .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
                    }
                }),
            ),
            Self::AsyncMut(thunk) => JsFunction::new(
                ctx.clone(),
                Async(move |this: This<Class<'js, HostInstance<C>>>, args: Rest<Value<'js>>| {
                    let ctx = this.0.ctx().clone();

                    let future = this
                        .0
                        .try_borrow_mut()
                        .map_err(Into::into)
                        .and_then(|mut guard| {
                            thunk(&mut (*guard).0, &Scope::detached(ctx.clone()), Args::new(args.0))
                        });

                    async move {
                        match future {
                            Ok(future) => future.await,
                            Err(error) => Err(error),
                        }
                        .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
                    }
                }),
            ),
        }
    }
}

#[repr(transparent)]
pub(crate) struct HostInstance<C: HostClass>(pub(crate) C);

impl<'js, C: HostClass> Trace<'js> for HostInstance<C> {
    fn trace<'a>(&self, _tracer: Tracer<'a, 'js>) {}
}

unsafe impl<'js, C: HostClass> JsLifetime<'js> for HostInstance<C> {
    type Changed<'to> = HostInstance<C>;
}

impl<'js, C: HostClass> JsClass<'js> for HostInstance<C> {
    const NAME: &'static str = C::NAME;

    type Mutable = Writable;

    fn prototype(ctx: &Ctx<'js>) -> rquickjs::Result<Option<JsObject<'js>>> {
        let prototype = JsObject::new(ctx.clone())?;
        let mut spec = ClassSpec::<C>::new();

        C::build(&mut spec);

        let ClassParts { methods, accessors, symbols, .. } = spec.into_parts();

        for (name, method) in methods {
            prototype.set(name, method.into_function(ctx)?)?;
        }

        for (name, get, set) in accessors {
            match (get, set) {
                (Some(get), Some(set)) => {
                    prototype.prop(
                        name,
                        Accessor::new(
                            move |ctx: Ctx<'js>,
                                  this: This<Class<'js, HostInstance<C>>>|
                                  -> rquickjs::Result<Value<'js>> {
                                let scope = Scope::detached(ctx.clone());
                                let guard = this.0.try_borrow()?;

                                get(&(*guard).0, &scope).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                            move |ctx: Ctx<'js>,
                                  this: This<Class<'js, HostInstance<C>>>,
                                  value: Value<'js>|
                                  -> rquickjs::Result<()> {
                                let scope = Scope::detached(ctx.clone());
                                let mut guard = this.0.try_borrow_mut()?;

                                set(&mut (*guard).0, &scope, value).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                        ),
                    )?;
                }
                (Some(get), None) => {
                    prototype.prop(
                        name,
                        Accessor::from(
                            move |ctx: Ctx<'js>,
                                  this: This<Class<'js, HostInstance<C>>>|
                                  -> rquickjs::Result<Value<'js>> {
                                let scope = Scope::detached(ctx.clone());
                                let guard = this.0.try_borrow()?;

                                get(&(*guard).0, &scope).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                        ),
                    )?;
                }
                (None, Some(set)) => {
                    prototype.prop(
                        name,
                        Accessor::new_set(
                            move |ctx: Ctx<'js>,
                                  this: This<Class<'js, HostInstance<C>>>,
                                  value: Value<'js>|
                                  -> rquickjs::Result<()> {
                                let scope = Scope::detached(ctx.clone());
                                let mut guard = this.0.try_borrow_mut()?;

                                set(&mut (*guard).0, &scope, value).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                        ),
                    )?;
                }
                (None, None) => {}
            }
        }

        for (symbol, method) in symbols {
            prototype.set(symbol.atom(ctx), method.into_function(ctx)?)?;
        }

        Ok(Some(prototype))
    }

    fn constructor(ctx: &Ctx<'js>) -> rquickjs::Result<Option<Constructor<'js>>> {
        Constructor::new_class::<HostInstance<C>, _, _>(
            ctx.clone(),
            |ctx: Ctx<'js>,
             args: Rest<Value<'js>>|
             -> rquickjs::Result<Class<'js, HostInstance<C>>> {
                Class::instance(
                    ctx.clone(),
                    HostInstance(
                        C::construct(&Scope::detached(ctx.clone()), Args::new(args.0))
                            .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))?,
                    ),
                )
            },
        )
        .map(Some)
    }
}

impl<C: HostClass> HostInstance<C> {
    pub(crate) fn into_guest<'js>(scope: &Scope<'js>, value: C) -> Result<Value<'js>, Error> {
        Ok(
            Class::instance(scope.ctx().clone(), HostInstance(value))
                .catch(scope.ctx())?
                .into_value()
        )
    }

    pub(crate) fn cloned<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<C, Error>
    where
        C: Clone,
    {
        Ok(
            (*OwnedBorrow::<HostInstance<C>>::from_js(scope.ctx(), value)?)
                .0
                .clone()
        )
    }

    pub(crate) fn export<'js>(scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        let constructor = Class::<HostInstance<C>>::create_constructor(scope.ctx())
            .catch(scope.ctx())?
            .ok_or_else(|| Error::unexpected("host class has no constructor"))?;

        let mut spec = ClassSpec::<C>::new();

        C::build(&mut spec);

        spec.into_parts()
            .statics
            .apply(scope, &constructor)?;

        Ok(constructor.into_value())
    }
}

/// A shared host-class borrow.
pub struct Ref<'js, C: HostClass>(OwnedBorrow<'js, HostInstance<C>>);

impl<'js, C: HostClass> Deref for Ref<'js, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &(*self.0).0
    }
}

/// An exclusive host-class borrow.
pub struct RefMut<'js, C: HostClass>(OwnedBorrowMut<'js, HostInstance<C>>);

impl<'js, C: HostClass> Deref for RefMut<'js, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &(*self.0).0
    }
}

impl<'js, C: HostClass> DerefMut for RefMut<'js, C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut (*self.0).0
    }
}

impl<'js, C: HostClass> FromGuestRef<'js> for C {
    type Ref = Ref<'js, C>;

    fn from_guest_ref(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Ref, Error> {
        Ok(Ref(OwnedBorrow::from_js(scope.ctx(), value)?))
    }
}

impl<'js, C: HostClass> FromGuestMut<'js> for C {
    type Mut = RefMut<'js, C>;

    fn from_guest_mut(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Mut, Error> {
        Ok(RefMut(OwnedBorrowMut::from_js(scope.ctx(), value)?))
    }
}

impl<C> ToGuest for C
where
    C: HostClass,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        HostInstance::<C>::into_guest(scope, self)
    }
}

impl<'js, C> ToGuestBound<'js> for C
where
    C: HostClass,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        HostInstance::<C>::into_guest(scope, self)
    }
}

impl<C> FromGuest for C
where
    C: HostClass + Clone,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        HostInstance::<C>::cloned(scope, value)
    }
}

impl<C> FromGuestBound for C
where
    C: HostClass + Clone,
{
    type Bound<'js> = Self;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        HostInstance::<C>::cloned(scope, value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        errors::Error,
        handle::{Module, Promise},
        host::{
            args::Args,
            callable::HostFn,
            class::{ClassSpec, HostClass},
            module::{Exports, HostModule},
            object::HostObject,
        },
        marshal::GuestType,
        runtime::{Runtime, Scope},
    };

    struct GuestTypeContract;

    impl GuestTypeContract {
        fn accepts<T>()
        where
            T: GuestType,
        {
        }
    }

    struct Vector2 {
        x: f64,
        y: f64,
    }

    impl HostClass for Vector2 {
        const NAME: &'static str = "Vector2";

        fn construct<'js>(scope: &Scope<'js>, args: Args<'js>) -> Result<Self, Error> {
            Ok(Self {
                x: args.get::<f64>(scope, 0)?,
                y: args.get::<f64>(scope, 1)?,
            })
        }

        fn build(spec: &mut ClassSpec<Self>) {
            spec.accessor::<_, _, _, f64>(
                "x",
                |vector, _scope| Ok(vector.x),
                |vector, _scope, value: f64| {
                    vector.x = value;

                    Ok(())
                },
            );
            spec.method("length", |vector, _scope, _args| {
                Ok((vector.x * vector.x + vector.y * vector.y).sqrt())
            });
            spec.async_method("lengthAsync", |vector, _scope, _args| {
                let (x, y) = (vector.x, vector.y);

                Ok(async move { Ok((x * x + y * y).sqrt()) })
            });
            spec.method("callback", |vector, _scope, _args| {
                let sum = vector.x + vector.y;

                Ok(HostFn::new(move |_scope, _args| Ok(sum)))
            });
            spec.iterable(|vector, _scope| Ok([vector.x, vector.y]));
            spec.statics(|statics| {
                statics.constant("DIMENSIONS", 2_i32);
            });
        }
    }

    struct Counter {
        n: i32,
    }

    impl HostClass for Counter {
        const NAME: &'static str = "Counter";

        fn construct<'js>(scope: &Scope<'js>, args: Args<'js>) -> Result<Self, Error> {
            Ok(Self { n: args.get::<i32>(scope, 0)? })
        }

        fn build(spec: &mut ClassSpec<Self>) {
            spec.method_mut("add", |counter, scope, args| {
                counter.n += args.get::<i32>(scope, 0)?;

                Ok(counter.n)
            });
            spec.method("value", |counter, _scope, _args| Ok(counter.n));
            spec.async_method_mut("addAsync", |counter, scope, args| {
                counter.n += args.get::<i32>(scope, 0)?;

                let value = counter.n;

                Ok(async move { Ok(value) })
            });
        }
    }

    struct MathHost;

    impl MathHost {
        async fn module(source: &str) -> Module {
            let runtime = Runtime::builder()
                .bind(Self)
                .build()
                .await
                .unwrap();

            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("t.js", source)
                .await
                .unwrap()
        }
    }

    impl HostModule for MathHost {
        fn name(&self) -> &str {
            "@host/math"
        }

        fn build(&self, exports: &mut Exports) {
            exports.class::<Vector2>();
            exports.class::<Counter>();
            exports.function("magnitude", |scope, args| {
                let vector = args.get_borrow::<Vector2>(scope, 0)?;

                Ok((vector.x * vector.x + vector.y * vector.y).sqrt())
            });
            exports.function("settings", |_scope, _args| {
                Ok(HostObject::build(|settings| {
                    settings.constant("unit", "px");
                }))
            });
        }
    }

    #[derive(Clone)]
    struct Coord {
        value: f64,
    }

    impl HostClass for Coord {
        const NAME: &'static str = "Coord";

        fn construct<'js>(scope: &Scope<'js>, args: Args<'js>) -> Result<Self, Error> {
            Ok(Self { value: args.get::<f64>(scope, 0)? })
        }

        fn build(spec: &mut ClassSpec<Self>) {
            spec.method("get", |coord, _scope, _args| Ok(coord.value));
        }
    }

    struct Bag;

    impl HostModule for Bag {
        fn name(&self) -> &str {
            "@host/bag"
        }

        fn build(&self, exports: &mut Exports) {
            exports.class::<Coord>();
            exports.function("make", |scope, args| Ok(Coord { value: args.get::<f64>(scope, 0)? }));
            exports.function("total", |scope, args| {
                Ok(args
                    .get::<Vec<Coord>>(scope, 0)?
                    .into_iter()
                    .map(|coord| coord.value)
                    .sum::<f64>())
            });
        }
    }

    #[test]
    fn cloneable_host_classes_are_guest_types() {
        GuestTypeContract::accepts::<Coord>();
    }

    #[tokio::test]
    async fn host_class_method() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export function run() { return new Vector2(3, 4).length(); }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, f64>(())
            .await
            .unwrap(),
            5.0,
        );
    }

    #[tokio::test]
    async fn host_class_mutable_state() {
        assert_eq!(
            MathHost::module(
                "import { Counter } from \"@host/math\";\n\
                 export function run() { const c = new Counter(10); c.add(5); return c.value(); }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, i32>(())
            .await
            .unwrap(),
            15,
        );
    }

    #[tokio::test]
    async fn host_class_async_method() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export async function run() { return await new Vector2(3, 4).lengthAsync(); }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, Promise<f64>>(())
            .await
            .unwrap()
            .await
            .unwrap(),
            5.0,
        );
    }

    #[tokio::test]
    async fn host_class_async_mutable_method() {
        assert_eq!(
            MathHost::module(
                "import { Counter } from \"@host/math\";\n\
                 export async function run() {\n\
                     const counter = new Counter(10);\n\
                     return await counter.addAsync(5);\n\
                 }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, Promise<i32>>(())
            .await
            .unwrap()
            .await
            .unwrap(),
            15,
        );
    }

    #[tokio::test]
    async fn host_class_accessor_reads_and_writes() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export function run() {\n\
                     const vector = new Vector2(3, 4);\n\
                     vector.x = 6;\n\
                     return vector.x;\n\
                 }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, f64>(())
            .await
            .unwrap(),
            6.0,
        );
    }

    #[tokio::test]
    async fn host_class_static_constant() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export function run() { return Vector2.DIMENSIONS; }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, i32>(())
            .await
            .unwrap(),
            2,
        );
    }

    #[tokio::test]
    async fn host_class_iterable() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export function run() { return [...new Vector2(3, 4)]; }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, Vec<f64>>(())
            .await
            .unwrap(),
            vec![3.0, 4.0],
        );
    }

    #[tokio::test]
    async fn host_function_returned_to_guest() {
        assert_eq!(
            MathHost::module(
                "import { Vector2 } from \"@host/math\";\n\
                 export function run() { return new Vector2(3, 4).callback()(); }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, f64>(())
            .await
            .unwrap(),
            7.0,
        );
    }

    #[tokio::test]
    async fn host_object_returned_to_guest() {
        assert_eq!(
            MathHost::module(
                "import { settings } from \"@host/math\";\n\
                 export function run() { return settings().unit; }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, String>(())
            .await
            .unwrap(),
            "px",
        );
    }

    #[tokio::test]
    async fn host_class_guest_to_host_borrow() {
        assert_eq!(
            MathHost::module(
                "import { Vector2, magnitude } from \"@host/math\";\n\
                 export function run() { return magnitude(new Vector2(3, 4)); }",
            )
            .await
            .function("run")
            .await
            .unwrap()
            .call::<_, f64>(())
            .await
            .unwrap(),
            5.0,
        );
    }

    #[tokio::test]
    async fn host_module_construct() {
        let runtime = Runtime::builder()
            .bind(MathHost)
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .host_module("@host/math")
                .await
                .unwrap()
                .class("Vector2")
                .await
                .unwrap()
                .construct((3.0, 4.0))
                .await
                .unwrap()
                .call::<_, f64>("length", ())
                .await
                .unwrap(),
            5.0,
        );
    }

    #[tokio::test]
    async fn instance_passed_as_argument() {
        let runtime = Runtime::builder()
            .bind(MathHost)
            .build()
            .await
            .unwrap();

        let guest = runtime.guest().build().await.unwrap();

        assert_eq!(
            guest
                .guest_module(
                    "t.js",
                    "export function measure(vector) { return vector.length(); }",
                )
                .await
                .unwrap()
                .function("measure")
                .await
                .unwrap()
                .call::<_, f64>((
                    guest
                        .host_module("@host/math")
                        .await
                        .unwrap()
                        .class("Vector2")
                        .await
                        .unwrap()
                        .construct((3.0, 4.0))
                        .await
                        .unwrap(),
                ))
                .await
                .unwrap(),
            5.0,
        );
    }

    #[tokio::test]
    async fn host_class_returns_and_receives_by_value() {
        let runtime = Runtime::builder()
            .bind(Bag)
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "bag.js",
                    "import { Coord, make, total } from \"@host/bag\";\n\
                     export function run() {\n\
                         return total([make(1), make(2), new Coord(3)]);\n\
                     }",
                )
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, f64>(())
                .await
                .unwrap(),
            6.0,
        );
    }

    #[tokio::test]
    async fn host_class_value_conversions_are_automatic() {
        let runtime = Runtime::builder()
            .bind(Bag)
            .build()
            .await
            .unwrap();
        let guest = runtime
            .guest()
            .build()
            .await
            .unwrap();

        assert_eq!(
            guest
                .host_module("@host/bag")
                .await
                .unwrap()
                .function("make")
                .await
                .unwrap()
                .call::<_, Coord>((42.0,))
                .await
                .unwrap()
                .value,
            42.0,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    scope
                        .host_module("@host/bag")
                        .await?
                        .function("total")?
                        .call::<_, f64>((
                            vec![Coord { value: 1.0 }, Coord { value: 2.0 }],
                        ))
                })
                .await
                .unwrap(),
            3.0,
        );
    }
}
