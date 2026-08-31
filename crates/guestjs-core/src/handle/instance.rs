use std::{marker::PhantomData, rc::Rc};

use rquickjs::{
    CatchResultExt, Function as JsFunction, Object as JsObject, Persistent, Value as JsValue,
    function::Args as JsArgs,
};

use crate::{
    errors::Error,
    handle::{BoundClass, Object},
    host::class::{HostClass, Ref, RefMut},
    marshal::{
        FromGuest, FromGuestBound, FromGuestMut, FromGuestRef, ToGuest, ToGuestArgs,
        ToGuestArgsBound, ToGuestBound,
    },
    runtime::{GuestContext, Scope},
};

/// An owned guest instance.
pub struct Instance<T = Object> {
    value: Persistent<JsObject<'static>>,
    context: Rc<GuestContext>,
    _identity: PhantomData<fn() -> T>,
}

impl<T> Instance<T> {
    pub(crate) fn new(value: Persistent<JsObject<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context, _identity: PhantomData }
    }

    /// Binds the instance to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundInstance<'js, T>, Error> {
        Ok(BoundInstance::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    pub fn as_untyped(&self) -> Instance {
        Instance::new(self.value.clone(), self.context.clone())
    }

    pub fn into_untyped(self) -> Instance {
        Instance::new(self.value, self.context)
    }

    pub async fn as_typed<C>(&self) -> Result<Instance<C>, Error>
    where
        C: HostClass,
    {
        Scope::with(&self.context, async move |scope| {
            drop(self.bind(&scope)?.borrow_as::<C>()?);

            Ok(Instance::new(self.value.clone(), self.context.clone()))
        })
        .await
    }

    pub async fn into_typed<C>(self) -> Result<Instance<C>, Error>
    where
        C: HostClass,
    {
        self.as_typed::<C>().await
    }

    /// Calls a guest method.
    pub async fn call<A, R>(&self, method: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(
                &scope,
                self.bind(&scope)?
                    .call_value(method, args.into_args(&scope)?)?,
            )
        })
        .await
    }

    /// Returns a property value.
    pub async fn get<R>(&self, property: &str) -> Result<R::Owned, Error>
    where
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(&scope, self.bind(&scope)?.get_value(property)?)
        })
        .await
    }

    /// Sets a property value.
    pub async fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuest,
    {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .set_value(property, value.to_guest(&scope)?)
        })
        .await
    }

    pub async fn borrow_as_with<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: HostClass,
        F: FnOnce(&C) -> R,
        R: 'static,
    {
        Scope::with(&self.context, async move |scope| {
            Ok(f(&*self.bind(&scope)?.borrow_as::<C>()?))
        })
        .await
    }

    pub async fn borrow_as_with_mut<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: HostClass,
        F: FnOnce(&mut C) -> R,
        R: 'static,
    {
        Scope::with(&self.context, async move |scope| {
            Ok(f(&mut *self
                .bind(&scope)?
                .borrow_as_mut::<C>()?))
        })
        .await
    }
}

impl<C> Instance<C>
where
    C: HostClass,
{
    pub async fn borrow_with<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&C) -> R,
        R: 'static,
    {
        self.borrow_as_with::<C, F, R>(f).await
    }

    pub async fn borrow_with_mut<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&mut C) -> R,
        R: 'static,
    {
        self.borrow_as_with_mut::<C, F, R>(f)
            .await
    }
}

impl<T> Clone for Instance<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone(), self.context.clone())
    }
}

impl<T> ToGuest for Instance<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js, T> ToGuestBound<'js> for Instance<T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl<T> FromGuest for Instance<T>
where
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Instance::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_object()
                    .ok_or_else(|| Error::conversion("expected an object"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<T> FromGuestBound for Instance<T> {
    type Bound<'js> = BoundInstance<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundInstance::new(
            value
                .into_object()
                .ok_or_else(|| Error::conversion("expected an object"))?,
            scope.clone(),
        ))
    }
}

/// A guest instance bound to a scope.
pub struct BoundInstance<'js, T = Object> {
    value: JsObject<'js>,
    scope: Scope<'js>,
    _identity: PhantomData<fn() -> T>,
}

impl<'js, T> BoundInstance<'js, T> {
    pub(crate) fn new(value: JsObject<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope, _identity: PhantomData }
    }

    fn call_value(&self, method: &str, mut args: JsArgs<'js>) -> Result<JsValue<'js>, Error> {
        args.this(self.value.clone())
            .catch(self.scope.ctx())?;

        self.value
            .get::<_, JsFunction>(method)
            .catch(self.scope.ctx())?
            .call_arg(args)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn get_value(&self, property: &str) -> Result<JsValue<'js>, Error> {
        self.value
            .get(property)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn set_value(&self, property: &str, value: JsValue<'js>) -> Result<(), Error> {
        self.value
            .set(property, value)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    pub fn as_untyped(&self) -> BoundInstance<'js> {
        BoundInstance::new(self.value.clone(), self.scope.clone())
    }

    pub fn into_untyped(self) -> BoundInstance<'js> {
        BoundInstance::new(self.value, self.scope)
    }

    pub fn as_typed<C>(&self) -> Result<BoundInstance<'js, C>, Error>
    where
        C: HostClass,
    {
        drop(self.borrow_as::<C>()?);

        Ok(BoundInstance::new(self.value.clone(), self.scope.clone()))
    }

    pub fn into_typed<C>(self) -> Result<BoundInstance<'js, C>, Error>
    where
        C: HostClass,
    {
        self.as_typed::<C>()
    }

    /// Calls a guest method.
    pub fn call<A, R>(&self, method: &str, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        R: FromGuestBound,
    {
        R::from_guest_bound(
            &self.scope,
            self.call_value(method, args.into_bound_args(&self.scope)?)?,
        )
    }

    /// Returns a property value.
    pub fn get<R>(&self, property: &str) -> Result<R::Bound<'js>, Error>
    where
        R: FromGuestBound,
    {
        R::from_guest_bound(&self.scope, self.get_value(property)?)
    }

    /// Sets a property value.
    pub fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuestBound<'js>,
    {
        self.set_value(property, value.to_guest_bound(&self.scope)?)
    }

    pub fn is_instance_of<R>(&self, class: &BoundClass<'js, R>) -> bool {
        self.value
            .is_instance_of(class.constructor())
    }

    pub fn borrow_as<C>(&self) -> Result<Ref<'js, C>, Error>
    where
        C: HostClass,
    {
        C::from_guest_ref(&self.scope, JsValue::from(self.value.clone()))
    }

    pub fn borrow_as_mut<C>(&self) -> Result<RefMut<'js, C>, Error>
    where
        C: HostClass,
    {
        C::from_guest_mut(&self.scope, JsValue::from(self.value.clone()))
    }

    /// Converts the instance into an owned handle.
    pub fn into_owned(self) -> Result<Instance<T>, Error> {
        Ok(Instance::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js, C> BoundInstance<'js, C>
where
    C: HostClass,
{
    pub fn borrow(&self) -> Result<Ref<'js, C>, Error> {
        self.borrow_as::<C>()
    }

    pub fn borrow_mut(&self) -> Result<RefMut<'js, C>, Error> {
        self.borrow_as_mut::<C>()
    }
}

impl<'js, T> ToGuestBound<'js> for BoundInstance<'js, T> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        errors::Error,
        handle::{Function, Object},
        host::{
            args::Args,
            class::{ClassSpec, HostClass},
            module::{Exports, HostModule},
        },
        runtime::{Runtime, Scope},
    };

    const INSTANCE_SOURCE: &str = r#"
        export function makeObject(value) {
            return {
                value,
            };
        }

        export class Holder {
            constructor(value) {
                this.value = value;
            }

            getObject() {
                return this.value;
            }

            getFunction() {
                return (offset) => this.value.value + offset;
            }

            replace(value) {
                this.value = value;
                return this.value;
            }
        }
    "#;

    struct Tally {
        hits: i32,
    }

    impl HostClass for Tally {
        const NAME: &'static str = "Tally";

        fn construct<'js>(scope: &Scope<'js>, args: Args<'js>) -> Result<Self, Error> {
            Ok(Self { hits: args.get::<i32>(scope, 0)? })
        }

        fn build(spec: &mut ClassSpec<Self>) {
            spec.method_mut("bump", |tally, _scope, _args| {
                tally.hits += 1;

                Ok(tally.hits)
            });
        }
    }

    struct Tallies;

    impl HostModule for Tallies {
        fn name(&self) -> &str {
            "@host/tallies"
        }

        fn build(&self, exports: &mut Exports) {
            exports.class::<Tally>();
        }
    }

    #[tokio::test]
    async fn construct_and_call_instance() {
        let counter = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "counter.js",
                "export class Counter {\n\
                     constructor(start) { this.n = start; }\n\
                     add(x) { this.n += x; return this.n; }\n\
                 }",
            )
            .await
            .unwrap()
            .class("Counter")
            .await
            .unwrap()
            .construct((10,))
            .await
            .unwrap();

        assert_eq!(
            counter
                .call::<_, i32>("add", (5,))
                .await
                .unwrap(),
            15
        );
        assert_eq!(counter.get::<i32>("n").await.unwrap(), 15);

        counter.set("n", 20).await.unwrap();

        assert_eq!(
            counter
                .call::<_, i32>("add", (5,))
                .await
                .unwrap(),
            25
        );
    }

    #[tokio::test]
    async fn batch_instance_operations() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        let class = guest
            .guest_module(
                "counter.js",
                "export class Counter {\n\
                     constructor(start) { this.n = start; }\n\
                     add(x) { this.n += x; return this.n; }\n\
                 }",
            )
            .await
            .unwrap()
            .class("Counter")
            .await
            .unwrap();

        assert_eq!(
            class
                .construct((2,))
                .await
                .unwrap()
                .get::<i32>("n")
                .await
                .unwrap(),
            2,
        );

        guest
            .scope(async move |scope| {
                let counter = class.bind(&scope)?.construct((10,))?;

                assert_eq!(counter.call::<_, i32>("add", (5,))?, 15);

                counter.set("n", 20)?;

                assert_eq!(counter.get::<i32>("n")?, 20);

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bound_instance_composes_handle_results_and_inputs() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("instances.js", INSTANCE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .class("Holder")
                .await
                .unwrap()
                .construct((module
                    .function("makeObject")
                    .await
                    .unwrap()
                    .call::<_, Object>((2,))
                    .await
                    .unwrap(),))
                .await
                .unwrap()
                .call::<_, Object>("getObject", ())
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            2,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    let module = module.bind(&scope)?;
                    let holder = module
                        .class("Holder")?
                        .construct((module
                            .function("makeObject")?
                            .call::<_, Object>((3,))?,))?;

                    assert_eq!(
                        holder
                            .get::<Object>("value")?
                            .get::<i32>("value")?,
                        3,
                    );
                    assert_eq!(
                        holder
                            .call::<_, Object>("getObject", ())?
                            .get::<i32>("value")?,
                        3,
                    );
                    assert_eq!(
                        holder
                            .call::<_, Function>("getFunction", ())?
                            .call::<_, i32>((4,))?,
                        7,
                    );

                    holder.set(
                        "value",
                        module
                            .function("makeObject")?
                            .call::<_, Object>((8,))?,
                    )?;
                    assert_eq!(
                        holder
                            .call::<_, Object>(
                                "replace",
                                (module
                                    .function("makeObject")?
                                    .call::<_, Object>((11,))?,),
                            )?
                            .get::<i32>("value")?,
                        11,
                    );

                    holder.into_owned()
                })
                .await
                .unwrap()
                .get::<Object>("value")
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            11,
        );
    }

    #[tokio::test]
    async fn instance_identity_does_not_change_behaviour() {
        let counter = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "counter.js",
                "export class Counter {\n\
                     constructor(start) { this.n = start; }\n\
                     add(x) { this.n += x; return this.n; }\n\
                 }",
            )
            .await
            .unwrap()
            .class("Counter")
            .await
            .unwrap()
            .construct((1,))
            .await
            .unwrap();

        assert_eq!(
            counter
                .call::<_, i32>("add", (2,))
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            counter
                .as_untyped()
                .call::<_, i32>("add", (2,))
                .await
                .unwrap(),
            5,
        );
    }

    #[tokio::test]
    async fn instance_adopts_a_host_class_identity() {
        let guest = Runtime::builder()
            .bind(Tallies)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let tally = guest
            .host_module("@host/tallies")
            .await
            .unwrap()
            .class("Tally")
            .await
            .unwrap()
            .construct((7,))
            .await
            .unwrap()
            .into_typed::<Tally>()
            .await
            .unwrap();

        assert_eq!(
            tally
                .call::<_, i32>("bump", ())
                .await
                .unwrap(),
            8
        );
        assert_eq!(
            tally
                .borrow_with(|tally| tally.hits)
                .await
                .unwrap(),
            8
        );

        tally
            .borrow_with_mut(|tally| tally.hits = 100)
            .await
            .unwrap();

        assert_eq!(
            tally
                .borrow_with(|tally| tally.hits)
                .await
                .unwrap(),
            100
        );
        assert!(
            guest
                .guest_module("plain.js", "export class Plain { constructor() {} }")
                .await
                .unwrap()
                .class("Plain")
                .await
                .unwrap()
                .construct(())
                .await
                .unwrap()
                .into_typed::<Tally>()
                .await
                .is_err(),
        );
    }
}
