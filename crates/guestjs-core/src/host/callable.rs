use std::{future::Future, pin::Pin};

use rquickjs::{
    CatchResultExt, Ctx, Exception, Function as JsFunction, Value,
    function::{Async, Rest},
};

use crate::{
    errors::Error,
    host::args::Args,
    marshal::{ToGuest, ToGuestBound},
    runtime::Scope,
};

pub(crate) type BoxFuture<'js> = Pin<Box<dyn Future<Output = Result<Value<'js>, Error>> + 'js>>;
type SyncThunk = Box<dyn for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<Value<'js>, Error>>;
type AsyncThunk = Box<dyn for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<BoxFuture<'js>, Error>>;

pub(crate) enum CallableBody {
    Sync(SyncThunk),
    Async(AsyncThunk),
}

impl CallableBody {
    pub(crate) fn sync<F, R>(f: F) -> Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        Self::Sync(Box::new(move |scope, args| f(scope, args)?.to_guest(scope)))
    }

    pub(crate) fn r#async<F, Fut, R>(f: F) -> Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest,
    {
        Self::Async(Box::new(move |scope, args| {
            let future = f(scope, args)?;
            let scope = scope.clone();

            Ok(Box::pin(async move { future.await?.to_guest(&scope) }))
        }))
    }

    pub(crate) fn into_function<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        match self {
            Self::Sync(thunk) => Ok(Value::from(
                JsFunction::new(
                    scope.ctx().clone(),
                    move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                        thunk(&Scope::detached(ctx.clone()), Args::new(args.0))
                            .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
                    },
                )
                .catch(scope.ctx())?,
            )),
            Self::Async(thunk) => Ok(Value::from(
                JsFunction::new(
                    scope.ctx().clone(),
                    Async(move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
                        let prepared = thunk(&Scope::detached(ctx.clone()), Args::new(args.0));

                        async move {
                            match prepared {
                                Ok(future) => future.await,
                                Err(error) => Err(error),
                            }
                            .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
                        }
                    }),
                )
                .catch(scope.ctx())?,
            )),
        }
    }
}

/// A Rust closure exposed as a guest function.
pub struct HostFn {
    body: CallableBody,
}

impl HostFn {
    /// Creates a synchronous host function.
    pub fn new<F, R>(f: F) -> Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<R, Error> + 'static,
        R: ToGuest,
    {
        Self { body: CallableBody::sync(f) }
    }

    /// Creates an asynchronous host function.
    pub fn new_async<F, Fut, R>(f: F) -> Self
    where
        F: for<'js> Fn(&Scope<'js>, Args<'js>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest,
    {
        Self { body: CallableBody::r#async(f) }
    }
}

impl ToGuest for HostFn {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.body.into_function(scope)
    }
}

impl<'js> ToGuestBound<'js> for HostFn {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        errors::Error,
        handle::Function,
        host::{Exports, HostModule},
        runtime::Runtime,
    };

    const CALLBACK_MODULE_SOURCE: &str = r#"
        import { invoke } from "@host/callback";

        export function run() {
            return invoke((left, right) => left + right);
        }
    "#;

    struct CallbackHostModule;

    impl HostModule for CallbackHostModule {
        fn name(&self) -> &str {
            "@host/callback"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("invoke", |scope, args| {
                assert!(matches!(
                    args
                        .get::<Function>(scope, 0)?
                        .into_owned(),
                    Err(Error::Unexpected { message, .. })
                        if message == "cannot build an owned guest handle on detached scope",
                ));

                args.get::<Function>(scope, 0)?
                    .call::<_, i32>((20, 22))
            });
        }
    }

    #[tokio::test]
    async fn bound_function_argument_operates_in_detached_callback() {
        let runtime = Runtime::builder()
            .bind(CallbackHostModule)
            .build()
            .await
            .unwrap();

        assert_eq!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("callback.js", CALLBACK_MODULE_SOURCE)
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, i32>(())
                .await
                .unwrap(),
            42,
        );
    }
}
