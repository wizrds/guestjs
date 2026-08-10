use std::future::Future;

use rquickjs::{
    CatchResultExt, Ctx, Exception, Object as JsObject, Value as JsValue,
    object::{Accessor, Property},
};

use crate::{
    errors::Error,
    host::{
        args::Args,
        callable::CallableBody,
        class::{HostClass, HostInstance},
    },
    marshal::{FromGuestBound, ToGuest},
    runtime::Scope,
};

pub(crate) type JsValueThunk = Box<dyn for<'js> Fn(&Scope<'js>) -> Result<JsValue<'js>, Error>>;
pub(crate) type SetThunk = Box<dyn for<'js> Fn(&Scope<'js>, JsValue<'js>) -> Result<(), Error>>;

pub(crate) enum Member {
    Callable(CallableBody),
    Constant(JsValueThunk),
    Property(JsValueThunk),
    Accessor {
        get: Option<JsValueThunk>,
        set: Option<SetThunk>,
    },
    Object(Namespace),
}

/// A collection of named host members.
pub struct Namespace {
    members: Vec<(String, Member)>,
}

impl Namespace {
    pub(crate) fn new() -> Self {
        Self { members: Vec::new() }
    }

    pub(crate) fn members(&self) -> impl Iterator<Item = &(String, Member)> {
        self.members.iter()
    }

    pub(crate) fn into_members(self) -> Vec<(String, Member)> {
        self.members
    }

    /// Defines a synchronous function.
    pub fn function<F, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.members
            .push((name.to_owned(), Member::Callable(CallableBody::sync(f))));

        self
    }

    /// Defines an asynchronous function.
    pub fn async_function<F, Fut, R>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest,
    {
        self.members
            .push((name.to_owned(), Member::Callable(CallableBody::r#async(f))));

        self
    }

    /// Defines a read-only value.
    pub fn constant<V>(&mut self, name: &str, value: V) -> &mut Self
    where
        V: ToGuest + Clone + 'static,
    {
        self.members.push((
            name.to_owned(),
            Member::Constant(Box::new(move |scope| value.clone().to_guest(scope))),
        ));

        self
    }

    /// Defines a writable value.
    pub fn property<V>(&mut self, name: &str, value: V) -> &mut Self
    where
        V: ToGuest + Clone + 'static,
    {
        self.members.push((
            name.to_owned(),
            Member::Property(Box::new(move |scope| value.clone().to_guest(scope))),
        ));

        self
    }

    /// Defines a getter.
    pub fn getter<F, R>(&mut self, name: &str, get: F) -> &mut Self
    where
        F: for<'js> Fn(&Scope<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        self.members.push((
            name.to_owned(),
            Member::Accessor {
                get: Some(Box::new(move |scope| get(scope)?.to_guest(scope))),
                set: None,
            },
        ));

        self
    }

    /// Defines a setter.
    pub fn setter<F, V>(&mut self, name: &str, set: F) -> &mut Self
    where
        F: for<'js> Fn(&Scope<'js>, V::Bound<'js>) -> Result<(), Error> + 'static,
        V: FromGuestBound,
    {
        self.members.push((
            name.to_owned(),
            Member::Accessor {
                get: None,
                set: Some(Box::new(move |scope, value| {
                    set(scope, V::from_guest_bound(scope, value)?)
                })),
            },
        ));

        self
    }

    /// Defines a getter and setter.
    pub fn accessor<G, S, R, V>(&mut self, name: &str, get: G, set: S) -> &mut Self
    where
        G: for<'js> Fn(&Scope<'js>) -> Result<R, Error> + 'static,
        S: for<'js> Fn(&Scope<'js>, V::Bound<'js>) -> Result<(), Error> + 'static,
        R: ToGuest,
        V: FromGuestBound,
    {
        self.members.push((
            name.to_owned(),
            Member::Accessor {
                get: Some(Box::new(move |scope| get(scope)?.to_guest(scope))),
                set: Some(Box::new(move |scope, value| {
                    set(scope, V::from_guest_bound(scope, value)?)
                })),
            },
        ));

        self
    }

    /// Defines a nested object.
    pub fn object<F>(&mut self, name: &str, build: F) -> &mut Self
    where
        F: FnOnce(&mut Namespace),
    {
        let mut nested = Namespace::new();

        build(&mut nested);

        self.members
            .push((name.to_owned(), Member::Object(nested)));

        self
    }

    /// Defines a host class.
    pub fn class<C>(&mut self) -> &mut Self
    where
        C: HostClass,
    {
        self.members.push((
            C::NAME.to_owned(),
            Member::Constant(Box::new(|scope| HostInstance::<C>::export(scope))),
        ));

        self
    }

    pub(crate) fn apply<'js>(
        self,
        scope: &Scope<'js>,
        object: &JsObject<'js>,
    ) -> Result<(), Error> {
        for (name, member) in self.members {
            member.apply(scope, object, &name)?;
        }

        Ok(())
    }
}

impl Member {
    pub(crate) fn apply<'js>(
        self,
        scope: &Scope<'js>,
        object: &JsObject<'js>,
        name: &str,
    ) -> Result<(), Error> {
        match self {
            Self::Callable(body) => {
                object
                    .set(name, body.into_function(scope)?)
                    .catch(scope.ctx())?;
            }
            Self::Constant(value) => {
                object
                    .prop(name, Property::from(value(scope)?))
                    .catch(scope.ctx())?;
            }
            Self::Property(value) => {
                object
                    .prop(name, Property::from(value(scope)?).writable())
                    .catch(scope.ctx())?;
            }
            Self::Accessor { get, set } => {
                Self::apply_accessor(scope, object, name, get, set)?;
            }
            Self::Object(nested) => {
                let child = JsObject::new(scope.ctx().clone()).catch(scope.ctx())?;

                nested.apply(scope, &child)?;

                object
                    .set(name, child)
                    .catch(scope.ctx())?;
            }
        }

        Ok(())
    }

    pub(crate) fn into_export_value<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        match self {
            Self::Callable(body) => body.into_function(scope),
            Self::Constant(value) | Self::Property(value) => value(scope),
            Self::Accessor { .. } => {
                Err(Error::unexpected("accessor members are not valid as module exports"))
            }
            Self::Object(nested) => {
                let object = JsObject::new(scope.ctx().clone()).catch(scope.ctx())?;

                nested.apply(scope, &object)?;

                Ok(object.into_value())
            }
        }
    }

    fn apply_accessor<'js>(
        scope: &Scope<'js>,
        object: &JsObject<'js>,
        name: &str,
        get: Option<JsValueThunk>,
        set: Option<SetThunk>,
    ) -> Result<(), Error> {
        match (get, set) {
            (Some(get), Some(set)) => {
                object
                    .prop(
                        name,
                        Accessor::new(
                            move |ctx: Ctx<'js>| -> rquickjs::Result<JsValue<'js>> {
                                get(&Scope::detached(ctx.clone())).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                            move |ctx: Ctx<'js>, value: JsValue<'js>| -> rquickjs::Result<()> {
                                set(&Scope::detached(ctx.clone()), value).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                        ),
                    )
                    .catch(scope.ctx())?;
            }
            (Some(get), None) => {
                object
                    .prop(
                        name,
                        Accessor::from(move |ctx: Ctx<'js>| -> rquickjs::Result<JsValue<'js>> {
                            get(&Scope::detached(ctx.clone()))
                                .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
                        }),
                    )
                    .catch(scope.ctx())?;
            }
            (None, Some(set)) => {
                object
                    .prop(
                        name,
                        Accessor::new_set(
                            move |ctx: Ctx<'js>, value: JsValue<'js>| -> rquickjs::Result<()> {
                                set(&Scope::detached(ctx.clone()), value).map_err(|error| {
                                    Exception::throw_message(&ctx, &error.to_string())
                                })
                            },
                        ),
                    )
                    .catch(scope.ctx())?;
            }
            (None, None) => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use crate::{
        host::module::{Exports, HostModule},
        runtime::Runtime,
    };

    struct StateHost {
        count: Rc<Cell<i32>>,
    }

    impl HostModule for StateHost {
        fn name(&self) -> &str {
            "@host/state"
        }

        fn build(&self, exports: &mut Exports) {
            let getter_count = self.count.clone();
            let setter_count = self.count.clone();

            exports.object("state", move |state| {
                state.accessor::<_, _, _, i32>(
                    "count",
                    move |_scope| Ok(getter_count.get()),
                    move |_scope, value: i32| {
                        setter_count.set(value);

                        Ok(())
                    },
                );
            });
        }
    }

    #[tokio::test]
    async fn namespace_accessor_reads_and_writes() {
        let runtime = Runtime::builder()
            .bind(StateHost { count: Rc::new(Cell::new(1)) })
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
                    "state.js",
                    "import { state } from \"@host/state\";\n\
                     export function run() { state.count = 7; return state.count; }",
                )
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, i32>(())
                .await
                .unwrap(),
            7,
        );
    }
}
