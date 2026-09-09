// Guest handles are bound to a single-threaded engine context, so no caller can ever need a
// Send bound on these futures.

#![allow(async_fn_in_trait)]

use rquickjs::{CatchResultExt, Function as JsFunction};

use crate::{
    errors::Error,
    handle::{BoundClass, BoundObject, Class, Object},
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestArgs, ToGuestArgsBound, ToGuestBound},
    runtime::Scope,
};

mod sealed {
    use std::rc::Rc;

    use rquickjs::{Constructor as JsConstructor, Function as JsFunction, Object as JsObject};

    use crate::{
        errors::Error,
        runtime::{GuestContext, Scope},
    };

    pub trait BoundHandle<'js> {
        fn js_object(&self) -> &JsObject<'js>;

        fn js_scope(&self) -> &Scope<'js>;
    }

    pub trait BoundCallable<'js>: BoundHandle<'js> {
        fn js_function(&self) -> &JsFunction<'js>;
    }

    pub trait BoundConstructor<'js>: BoundHandle<'js> {
        type Result;

        fn js_constructor(&self) -> &JsConstructor<'js>;
    }

    pub trait OwnedHandle {
        fn guest_context(&self) -> &Rc<GuestContext>;

        fn bind_object<'js>(&self, scope: &Scope<'js>) -> Result<JsObject<'js>, Error>;
    }

    pub trait OwnedCallable: OwnedHandle {
        fn bind_function<'js>(&self, scope: &Scope<'js>) -> Result<JsFunction<'js>, Error>;
    }

    pub trait OwnedConstructor: OwnedHandle {
        type Result;

        fn bind_constructor<'js>(&self, scope: &Scope<'js>) -> Result<JsConstructor<'js>, Error>;
    }
}

pub(crate) use sealed::{
    BoundCallable, BoundConstructor, BoundHandle, OwnedCallable, OwnedConstructor, OwnedHandle,
};

/// Property and prototype operations on a scope bound guest object.
pub trait BoundObjectProtocol<'js>: BoundHandle<'js> {
    /// Returns a property value.
    fn get<R>(&self, property: &str) -> Result<R::Bound<'js>, Error>
    where
        R: FromGuestBound,
    {
        R::from_guest_bound(
            self.js_scope(),
            self.js_object()
                .get(property)
                .catch(self.js_scope().ctx())?,
        )
    }

    /// Sets a property value.
    fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuestBound<'js>,
    {
        self.js_object()
            .set(property, value.to_guest_bound(self.js_scope())?)
            .catch(self.js_scope().ctx())
            .map_err(Into::into)
    }

    /// Reports whether a property is reachable on the object or its prototype chain.
    fn has(&self, property: &str) -> Result<bool, Error> {
        self.js_object()
            .contains_key(property)
            .catch(self.js_scope().ctx())
            .map_err(Into::into)
    }

    /// Deletes an own property.
    fn delete(&self, property: &str) -> Result<(), Error> {
        self.js_object()
            .remove(property)
            .catch(self.js_scope().ctx())
            .map_err(Into::into)
    }

    /// Returns the own enumerable string keys of the object.
    fn keys(&self) -> Result<Vec<String>, Error> {
        self.js_object()
            .keys::<String>()
            .collect::<Result<Vec<_>, _>>()
            .catch(self.js_scope().ctx())
            .map_err(Into::into)
    }

    /// Calls a method of the object with the object as the receiver.
    fn call_method<A, R>(&self, method: &str, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        R: FromGuestBound,
    {
        let mut args = args.into_bound_args(self.js_scope())?;

        args.this(self.js_object().clone())
            .catch(self.js_scope().ctx())?;

        R::from_guest_bound(
            self.js_scope(),
            self.js_object()
                .get::<_, JsFunction>(method)
                .catch(self.js_scope().ctx())?
                .call_arg(args)
                .catch(self.js_scope().ctx())?,
        )
    }

    /// Returns the prototype of the object, if it has one.
    fn prototype(&self) -> Option<BoundObject<'js>> {
        self.js_object()
            .get_prototype()
            .map(|prototype| BoundObject::new(prototype, self.js_scope().clone()))
    }

    /// Reports whether the object is an instance of a class.
    fn is_instance_of<R>(&self, class: &BoundClass<'js, R>) -> bool {
        self.js_object()
            .is_instance_of(class.js_constructor())
    }
}

impl<'js, T> BoundObjectProtocol<'js> for T where T: BoundHandle<'js> {}

/// Property and prototype operations on an owned guest object.
pub trait ObjectProtocol: OwnedHandle {
    /// Returns a property value.
    async fn get<R>(&self, property: &str) -> Result<R::Owned, Error>
    where
        R: FromGuest,
    {
        Scope::with(self.guest_context(), async move |scope| {
            R::from_guest(
                &scope,
                self.bind_object(&scope)?
                    .get(property)
                    .catch(scope.ctx())?,
            )
        })
        .await
    }

    /// Sets a property value.
    async fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuest,
    {
        Scope::with(self.guest_context(), async move |scope| {
            self.bind_object(&scope)?
                .set(property, value.to_guest(&scope)?)
                .catch(scope.ctx())
                .map_err(Into::into)
        })
        .await
    }

    /// Reports whether a property is reachable on the object or its prototype chain.
    async fn has(&self, property: &str) -> Result<bool, Error> {
        Scope::with(self.guest_context(), async move |scope| {
            self.bind_object(&scope)?
                .contains_key(property)
                .catch(scope.ctx())
                .map_err(Into::into)
        })
        .await
    }

    /// Deletes an own property.
    async fn delete(&self, property: &str) -> Result<(), Error> {
        Scope::with(self.guest_context(), async move |scope| {
            self.bind_object(&scope)?
                .remove(property)
                .catch(scope.ctx())
                .map_err(Into::into)
        })
        .await
    }

    /// Returns the own enumerable string keys of the object.
    async fn keys(&self) -> Result<Vec<String>, Error> {
        Scope::with(self.guest_context(), async move |scope| {
            self.bind_object(&scope)?
                .keys::<String>()
                .collect::<Result<Vec<_>, _>>()
                .catch(scope.ctx())
                .map_err(Into::into)
        })
        .await
    }

    /// Calls a method of the object with the object as the receiver.
    async fn call_method<A, R>(&self, method: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
        R: FromGuest,
    {
        Scope::with(self.guest_context(), async move |scope| {
            let object = self.bind_object(&scope)?;
            let mut args = args.into_args(&scope)?;

            args.this(object.clone())
                .catch(scope.ctx())?;

            R::from_guest(
                &scope,
                object
                    .get::<_, JsFunction>(method)
                    .catch(scope.ctx())?
                    .call_arg(args)
                    .catch(scope.ctx())?,
            )
        })
        .await
    }

    /// Returns the prototype of the object, if it has one.
    async fn prototype(&self) -> Result<Option<Object>, Error> {
        Scope::with(self.guest_context(), async move |scope| {
            self.bind_object(&scope)?
                .get_prototype()
                .map(|prototype| BoundObject::new(prototype, scope.clone()).into_owned())
                .transpose()
        })
        .await
    }

    /// Reports whether the object is an instance of a class.
    async fn is_instance_of<R>(&self, class: &Class<R>) -> Result<bool, Error> {
        Scope::with(self.guest_context(), async move |scope| {
            Ok(self
                .bind_object(&scope)?
                .is_instance_of(&class.bind_constructor(&scope)?))
        })
        .await
    }
}

impl<T> ObjectProtocol for T where T: OwnedHandle {}

/// Invocation of a scope bound guest function.
pub trait BoundCallableProtocol<'js>: BoundCallable<'js> {
    /// Calls the function.
    fn call<A, R>(&self, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        R: FromGuestBound,
    {
        R::from_guest_bound(
            self.js_scope(),
            self.js_function()
                .call_arg(args.into_bound_args(self.js_scope())?)
                .catch(self.js_scope().ctx())?,
        )
    }
}

impl<'js, T> BoundCallableProtocol<'js> for T where T: BoundCallable<'js> {}

/// Invocation of an owned guest function.
pub trait CallableProtocol: OwnedCallable {
    /// Calls the function.
    async fn call<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
        R: FromGuest,
    {
        Scope::with(self.guest_context(), async move |scope| {
            R::from_guest(
                &scope,
                self.bind_function(&scope)?
                    .call_arg(args.into_args(&scope)?)
                    .catch(scope.ctx())?,
            )
        })
        .await
    }
}

impl<T> CallableProtocol for T where T: OwnedCallable {}

/// Construction from a scope bound guest class.
pub trait BoundConstructorProtocol<'js>: BoundConstructor<'js> {
    /// Constructs an instance, converting the result to an explicit type.
    fn construct_as<A, O>(&self, args: A) -> Result<O::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        O: FromGuestBound,
    {
        O::from_guest_bound(
            self.js_scope(),
            self.js_constructor()
                .construct_args(args.into_bound_args(self.js_scope())?)
                .catch(self.js_scope().ctx())?,
        )
    }

    /// Constructs an instance.
    fn construct<A>(&self, args: A) -> Result<<Self::Result as FromGuestBound>::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        Self::Result: FromGuestBound,
    {
        self.construct_as::<A, Self::Result>(args)
    }
}

impl<'js, T> BoundConstructorProtocol<'js> for T where T: BoundConstructor<'js> {}

/// Construction from an owned guest class.
pub trait ConstructorProtocol: OwnedConstructor {
    /// Constructs an instance, converting the result to an explicit type.
    async fn construct_as<A, O>(&self, args: A) -> Result<O::Owned, Error>
    where
        A: ToGuestArgs,
        O: FromGuest,
    {
        Scope::with(self.guest_context(), async move |scope| {
            O::from_guest(
                &scope,
                self.bind_constructor(&scope)?
                    .construct_args(args.into_args(&scope)?)
                    .catch(scope.ctx())?,
            )
        })
        .await
    }

    /// Constructs an instance.
    async fn construct<A>(&self, args: A) -> Result<<Self::Result as FromGuest>::Owned, Error>
    where
        A: ToGuestArgs,
        Self::Result: FromGuest,
    {
        self.construct_as::<A, Self::Result>(args)
            .await
    }
}

impl<T> ConstructorProtocol for T where T: OwnedConstructor {}
