use std::rc::Rc;

use rquickjs::{
    CatchResultExt, Constructor as JsConstructor, Function as JsFunction, Object as JsObject,
    Persistent, Value as JsValue,
};

use crate::{
    errors::Error,
    handle::{
        BoundClass, BoundFunction, BoundHandle, BoundObject, Class, Function, Instance, Object,
        OwnedHandle,
    },
    marshal::ToGuestBound,
    runtime::{GuestContext, Scope},
};

/// An owned guest module.
#[derive(Clone)]
pub struct Module {
    namespace: Persistent<JsObject<'static>>,
    context: Rc<GuestContext>,
}

impl Module {
    pub(crate) fn new(namespace: Persistent<JsObject<'static>>, context: Rc<GuestContext>) -> Self {
        Self { namespace, context }
    }

    /// Binds the module to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundModule<'js>, Error> {
        Ok(BoundModule::new(
            self.namespace
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    /// Returns an exported function.
    pub async fn function(&self, name: &str) -> Result<Function, Error> {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .function(name)?
                .into_owned()
        })
        .await
    }

    /// Returns an exported object.
    pub async fn object(&self, name: &str) -> Result<Object, Error> {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .object(name)?
                .into_owned()
        })
        .await
    }

    /// Returns an exported class.
    pub async fn class(&self, name: &str) -> Result<Class, Error> {
        self.class_as::<Instance>(name).await
    }

    pub async fn class_as<R>(&self, name: &str) -> Result<Class<R>, Error>
    where
        R: 'static,
    {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .class_as::<R>(name)?
                .into_owned()
        })
        .await
    }
}

impl OwnedHandle for Module {
    fn guest_context(&self) -> &Rc<GuestContext> {
        &self.context
    }

    fn bind_object<'js>(&self, scope: &Scope<'js>) -> Result<JsObject<'js>, Error> {
        self.namespace
            .clone()
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
    }
}

/// A guest module bound to a scope.
pub struct BoundModule<'js> {
    namespace: JsObject<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundModule<'js> {
    pub(crate) fn new(namespace: JsObject<'js>, scope: Scope<'js>) -> Self {
        Self { namespace, scope }
    }

    /// Returns an exported function.
    pub fn function(&self, name: &str) -> Result<BoundFunction<'js>, Error> {
        Ok(BoundFunction::new(
            self.namespace
                .get::<_, JsFunction>(name)
                .catch(self.scope.ctx())?,
            self.scope.clone(),
        ))
    }

    /// Returns an exported object.
    pub fn object(&self, name: &str) -> Result<BoundObject<'js>, Error> {
        Ok(BoundObject::new(
            self.namespace
                .get::<_, JsObject>(name)
                .catch(self.scope.ctx())?,
            self.scope.clone(),
        ))
    }

    /// Returns an exported class.
    pub fn class(&self, name: &str) -> Result<BoundClass<'js>, Error> {
        self.class_as::<Instance>(name)
    }

    pub fn class_as<R>(&self, name: &str) -> Result<BoundClass<'js, R>, Error> {
        Ok(BoundClass::new(
            self.namespace
                .get::<_, JsConstructor>(name)
                .catch(self.scope.ctx())?,
            self.scope.clone(),
        ))
    }

    /// Converts the module into an owned handle.
    pub fn into_owned(self) -> Result<Module, Error> {
        Ok(Module::new(
            Persistent::save(self.scope.ctx(), self.namespace),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js> BoundHandle<'js> for BoundModule<'js> {
    fn js_object(&self) -> &JsObject<'js> {
        &self.namespace
    }

    fn js_scope(&self) -> &Scope<'js> {
        &self.scope
    }
}

impl<'js> ToGuestBound<'js> for BoundModule<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.namespace))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{
            BoundCallableProtocol,
            BoundConstructorProtocol,
            BoundObjectProtocol,
            CallableProtocol,
            Function,
            ObjectProtocol,
        },
        runtime::Runtime,
    };

    const MODULE_SOURCE: &str = r#"
        export function add(a, b) {
            return a + b;
        }

        export const settings = {
            unit: "px",
        };

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
    async fn bound_exports_remain_in_scope() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("exports.js", MODULE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .function("add")
                .await
                .unwrap()
                .call::<_, i32>((1, 1))
                .await
                .unwrap(),
            2,
        );

        guest
            .scope(async move |scope| {
                let module = module.bind(&scope)?;

                assert_eq!(
                    module
                        .function("add")?
                        .call::<_, i32>((2, 3))?,
                    5
                );
                assert_eq!(
                    module
                        .object("settings")?
                        .get::<String>("unit")?,
                    "px"
                );
                assert_eq!(
                    module
                        .class("Counter")?
                        .construct((4,))?
                        .call_method::<_, i32>("increment", ())?,
                    5,
                );

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn promoted_module_remains_operational() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("promotion.js", MODULE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .get::<Function>("add")
                .await
                .unwrap()
                .call::<_, i32>((1, 2))
                .await
                .unwrap(),
            3,
        );
        assert_eq!(
            guest
                .scope(async move |scope| { module.bind(&scope)?.into_owned() })
                .await
                .unwrap()
                .function("add")
                .await
                .unwrap()
                .call::<_, i32>((5, 7))
                .await
                .unwrap(),
            12,
        );
    }
}
