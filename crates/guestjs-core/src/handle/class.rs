use std::{marker::PhantomData, rc::Rc};

use rquickjs::{
    CatchResultExt, Constructor as JsConstructor, Persistent, Value as JsValue,
    function::Args as JsArgs,
};

use crate::{
    errors::Error,
    handle::Instance,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestArgs, ToGuestArgsBound, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest class.
pub struct Class<R = Instance> {
    value: Persistent<JsConstructor<'static>>,
    context: Rc<GuestContext>,
    _result: PhantomData<fn() -> R>,
}

impl<R> Class<R> {
    pub(crate) fn new(
        value: Persistent<JsConstructor<'static>>,
        context: Rc<GuestContext>,
    ) -> Self {
        Self { value, context, _result: PhantomData }
    }

    /// Binds the class to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundClass<'js, R>, Error> {
        Ok(BoundClass::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    pub fn with_result<O>(&self) -> Class<O> {
        Class::new(self.value.clone(), self.context.clone())
    }

    pub fn into_result<O>(self) -> Class<O> {
        Class::new(self.value, self.context)
    }

    pub async fn construct_as<A, O>(&self, args: A) -> Result<O::Owned, Error>
    where
        A: ToGuestArgs,
        O: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            O::from_guest(
                &scope,
                self.bind(&scope)?
                    .construct_value(args.into_args(&scope)?)?,
            )
        })
        .await
    }
}

impl<R> Class<R>
where
    R: FromGuest,
{
    /// Constructs a guest instance.
    pub async fn construct<A>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
    {
        self.construct_as::<A, R>(args).await
    }
}

impl<R> Clone for Class<R> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone(), self.context.clone())
    }
}

impl<R> ToGuest for Class<R> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js, R> ToGuestBound<'js> for Class<R> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl<R> FromGuest for Class<R>
where
    R: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Class::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_constructor()
                    .ok_or_else(|| Error::conversion("expected a class"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<R> FromGuestBound for Class<R> {
    type Bound<'js> = BoundClass<'js, R>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundClass::new(
            value
                .into_constructor()
                .ok_or_else(|| Error::conversion("expected a class"))?,
            scope.clone(),
        ))
    }
}

/// A guest class bound to a scope.
pub struct BoundClass<'js, R = Instance> {
    value: JsConstructor<'js>,
    scope: Scope<'js>,
    _result: PhantomData<fn() -> R>,
}

impl<'js, R> BoundClass<'js, R> {
    pub(crate) fn new(value: JsConstructor<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope, _result: PhantomData }
    }

    pub(crate) fn constructor(&self) -> &JsConstructor<'js> {
        &self.value
    }

    fn construct_value(&self, args: JsArgs<'js>) -> Result<JsValue<'js>, Error> {
        self.value
            .construct_args(args)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    pub fn with_result<O>(&self) -> BoundClass<'js, O> {
        BoundClass::new(self.value.clone(), self.scope.clone())
    }

    pub fn into_result<O>(self) -> BoundClass<'js, O> {
        BoundClass::new(self.value, self.scope)
    }

    pub fn construct_as<A, O>(&self, args: A) -> Result<O::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        O: FromGuestBound,
    {
        O::from_guest_bound(&self.scope, self.construct_value(args.into_bound_args(&self.scope)?)?)
    }

    /// Converts the class into an owned handle.
    pub fn into_owned(self) -> Result<Class<R>, Error> {
        Ok(Class::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js, R> BoundClass<'js, R>
where
    R: FromGuestBound,
{
    /// Constructs a guest instance.
    pub fn construct<A>(&self, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
    {
        self.construct_as::<A, R>(args)
    }
}

impl<'js, R> ToGuestBound<'js> for BoundClass<'js, R> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{handle::Object, runtime::Runtime};

    const CLASS_SOURCE: &str = r#"
        export class Counter {
            constructor(value) {
                this.value = value;
            }

            increment() {
                return ++this.value;
            }
        }
    "#;

    #[tokio::test]
    async fn promoted_class_constructs_owned_instances() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("classes.js", CLASS_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .class("Counter")
                .await
                .unwrap()
                .construct((1,))
                .await
                .unwrap()
                .call::<_, i32>("increment", ())
                .await
                .unwrap(),
            2,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    module
                        .bind(&scope)?
                        .class("Counter")?
                        .into_owned()
                })
                .await
                .unwrap()
                .construct((9,))
                .await
                .unwrap()
                .call::<_, i32>("increment", ())
                .await
                .unwrap(),
            10,
        );
    }

    #[tokio::test]
    async fn class_result_type_can_be_retyped_and_overridden() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("classes.js", CLASS_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .class("Counter")
                .await
                .unwrap()
                .construct_as::<_, Object>((3,))
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            3,
        );
        assert_eq!(
            module
                .class("Counter")
                .await
                .unwrap()
                .into_result::<Object>()
                .construct((4,))
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            4,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    let class = module.bind(&scope)?.class("Counter")?;

                    assert!(
                        class
                            .construct((5,))?
                            .is_instance_of(&class)
                    );

                    class
                        .with_result::<Object>()
                        .construct((6,))?
                        .get::<i32>("value")
                })
                .await
                .unwrap(),
            6,
        );
    }

    #[tokio::test]
    async fn module_returns_a_typed_class() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("classes.js", CLASS_SOURCE)
                .await
                .unwrap()
                .class_as::<Object>("Counter")
                .await
                .unwrap()
                .construct((8,))
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            8,
        );
    }
}
