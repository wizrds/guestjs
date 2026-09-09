use std::rc::Rc;

use rquickjs::{CatchResultExt, Object as JsObject, Persistent, Value as JsValue};

use crate::{
    errors::Error,
    handle::{BoundHandle, OwnedHandle},
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest object.
#[derive(Clone)]
pub struct Object {
    value: Persistent<JsObject<'static>>,
    context: Rc<GuestContext>,
}

impl Object {
    pub(crate) fn new(value: Persistent<JsObject<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context }
    }

    /// Binds the object to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundObject<'js>, Error> {
        Ok(BoundObject::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }
}

impl OwnedHandle for Object {
    fn guest_context(&self) -> &Rc<GuestContext> {
        &self.context
    }

    fn bind_object<'js>(&self, scope: &Scope<'js>) -> Result<JsObject<'js>, Error> {
        self.value
            .clone()
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
    }
}

impl ToGuest for Object {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js> ToGuestBound<'js> for Object {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Object {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Object::new(
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

impl FromGuestBound for Object {
    type Bound<'js> = BoundObject<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundObject::new(
            value
                .into_object()
                .ok_or_else(|| Error::conversion("expected an object"))?,
            scope.clone(),
        ))
    }
}

/// A guest object bound to a scope.
pub struct BoundObject<'js> {
    value: JsObject<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundObject<'js> {
    pub(crate) fn new(value: JsObject<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    /// Converts the object into an owned handle.
    pub fn into_owned(self) -> Result<Object, Error> {
        Ok(Object::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js> BoundHandle<'js> for BoundObject<'js> {
    fn js_object(&self) -> &JsObject<'js> {
        &self.value
    }

    fn js_scope(&self) -> &Scope<'js> {
        &self.scope
    }
}

impl<'js> ToGuestBound<'js> for BoundObject<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{
            BoundCallableProtocol,
            BoundObjectProtocol,
            CallableProtocol,
            Function,
            ObjectProtocol,
        },
        runtime::Runtime,
    };

    const OBJECT_SOURCE: &str = r#"
        export const holder = {
            value: 2,
        };

        export function makeFunction() {
            return (value) => value * 3;
        }
    "#;

    const PROTOCOL_SOURCE: &str = r#"
        export const point = {
            x: 3,
            y: 4,

            sum() {
                return this.x + this.y;
            },
        };
    "#;

    #[tokio::test]
    async fn bound_object_composes_with_function_handles() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("objects.js", OBJECT_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .object("holder")
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
                    let holder = module.object("holder")?;

                    holder.set(
                        "callback",
                        module
                            .function("makeFunction")?
                            .call::<_, Function>(())?,
                    )?;

                    assert_eq!(
                        holder
                            .get::<Function>("callback")?
                            .call::<_, i32>((4,))?,
                        12,
                    );

                    module.object("holder")?.into_owned()
                })
                .await
                .unwrap()
                .get::<Function>("callback")
                .await
                .unwrap()
                .call::<_, i32>((5,))
                .await
                .unwrap(),
            15,
        );
    }

    #[tokio::test]
    async fn object_protocol_covers_the_property_surface() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let point = guest
            .guest_module("protocol.js", PROTOCOL_SOURCE)
            .await
            .unwrap()
            .object("point")
            .await
            .unwrap();

        assert_eq!(point.call_method::<_, i32>("sum", ()).await.unwrap(), 7);
        assert!(point.has("x").await.unwrap());
        assert!(!point.has("z").await.unwrap());
        assert_eq!(point.keys().await.unwrap(), ["x", "y", "sum"]);

        point.delete("y").await.unwrap();

        assert!(!point.has("y").await.unwrap());
        assert!(point.prototype().await.unwrap().is_some());
    }
}
