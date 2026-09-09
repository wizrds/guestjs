use std::{marker::PhantomData, rc::Rc};

use rquickjs::{
    CatchResultExt, Constructor as JsConstructor, Object as JsObject, Persistent, Value as JsValue,
};

use crate::{
    errors::Error,
    handle::{
        BoundConstructor,
        BoundHandle,
        BoundObjectProtocol,
        Instance,
        Object,
        OwnedConstructor,
        OwnedHandle,
    },
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
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

    /// Reports whether the class is a strict subclass of another class.
    pub async fn is_subclass_of<O>(&self, class: &Class<O>) -> Result<bool, Error> {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .is_subclass_of(&class.bind(&scope)?)
        })
        .await
    }
}

impl<R> OwnedHandle for Class<R> {
    fn guest_context(&self) -> &Rc<GuestContext> {
        &self.context
    }

    fn bind_object<'js>(&self, scope: &Scope<'js>) -> Result<JsObject<'js>, Error> {
        Ok(self
            .value
            .clone()
            .restore(scope.ctx())
            .catch(scope.ctx())?
            .into_inner()
            .into_inner())
    }
}

impl<R> OwnedConstructor for Class<R> {
    type Result = R;

    fn bind_constructor<'js>(&self, scope: &Scope<'js>) -> Result<JsConstructor<'js>, Error> {
        self.value
            .clone()
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
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

    pub fn with_result<O>(&self) -> BoundClass<'js, O> {
        BoundClass::new(self.value.clone(), self.scope.clone())
    }

    pub fn into_result<O>(self) -> BoundClass<'js, O> {
        BoundClass::new(self.value, self.scope)
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

    /// Reports whether the class is a strict subclass of another class.
    pub fn is_subclass_of<O>(&self, class: &BoundClass<'js, O>) -> Result<bool, Error> {
        Ok(self
            .get::<Object>("prototype")?
            .is_instance_of(class))
    }
}

impl<'js, R> BoundHandle<'js> for BoundClass<'js, R> {
    fn js_object(&self) -> &JsObject<'js> {
        &self.value
    }

    fn js_scope(&self) -> &Scope<'js> {
        &self.scope
    }
}

impl<'js, R> BoundConstructor<'js> for BoundClass<'js, R> {
    type Result = R;

    fn js_constructor(&self) -> &JsConstructor<'js> {
        &self.value
    }
}

impl<'js, R> ToGuestBound<'js> for BoundClass<'js, R> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{
            BoundConstructorProtocol,
            BoundObjectProtocol,
            ConstructorProtocol,
            Object,
            ObjectProtocol,
        },
        runtime::Runtime,
    };

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

    const HIERARCHY_SOURCE: &str = r#"
        export class Shape {
            static kind = "shape";

            static describe() {
                return this.kind;
            }
        }

        export class Circle extends Shape {
            static kind = "circle";
        }

        export class Unrelated {}
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
                .call_method::<_, i32>("increment", ())
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
                .call_method::<_, i32>("increment", ())
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

    #[tokio::test]
    async fn class_reads_static_members() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("hierarchy.js", HIERARCHY_SOURCE)
            .await
            .unwrap();
        let circle = module
            .class("Circle")
            .await
            .unwrap();

        assert_eq!(circle.get::<String>("kind").await.unwrap(), "circle");
        assert!(circle.has("describe").await.unwrap());
        assert_eq!(
            circle
                .call_method::<_, String>("describe", ())
                .await
                .unwrap(),
            "circle",
        );
        assert!(
            circle
                .keys()
                .await
                .unwrap()
                .contains(&String::from("kind"))
        );

        guest
            .scope(async move |scope| {
                let circle = module
                    .bind(&scope)?
                    .class("Circle")?;

                assert_eq!(circle.get::<String>("kind")?, "circle");
                assert_eq!(circle.call_method::<_, String>("describe", ())?, "circle");

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn class_reports_strict_subclasses() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("hierarchy.js", HIERARCHY_SOURCE)
            .await
            .unwrap();
        let shape = module.class("Shape").await.unwrap();
        let circle = module.class("Circle").await.unwrap();
        let unrelated = module
            .class("Unrelated")
            .await
            .unwrap();

        assert!(circle.is_subclass_of(&shape).await.unwrap());
        assert!(!shape.is_subclass_of(&circle).await.unwrap());
        assert!(!shape.is_subclass_of(&shape).await.unwrap());
        assert!(!unrelated.is_subclass_of(&shape).await.unwrap());

        guest
            .scope(async move |scope| {
                let module = module.bind(&scope)?;

                assert!(
                    module
                        .class("Circle")?
                        .is_subclass_of(&module.class("Shape")?)?
                );
                assert!(
                    !module
                        .class("Shape")?
                        .is_subclass_of(&module.class("Circle")?)?
                );

                Ok(())
            })
            .await
            .unwrap();
    }
}
