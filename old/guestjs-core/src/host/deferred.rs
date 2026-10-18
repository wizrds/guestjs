use rquickjs::Value as JsValue;

use crate::{errors::Error, handle::value::Value, marshal::ToGuest, runtime::Scope};

/// Defers a guest conversion until a [`Scope`](crate::runtime::Scope) is available.
pub struct Deferred<F>
where
    F: for<'js> FnOnce(&Scope<'js>) -> Result<Value, Error>,
{
    callback: F,
}

impl<F> Deferred<F>
where
    F: for<'js> FnOnce(&Scope<'js>) -> Result<Value, Error>,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ToGuest for Deferred<F>
where
    F: for<'js> FnOnce(&Scope<'js>) -> Result<Value, Error>,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        (self.callback)(scope)?.to_guest(scope)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{CallableProtocol, Promise, Value},
        host::{Deferred, Exports, HostModule},
        runtime::Runtime,
    };

    struct DeferredHost;

    impl HostModule for DeferredHost {
        fn name(&self) -> &str {
            "@host/deferred"
        }

        fn build(&self, exports: &mut Exports) {
            exports.async_function("carry", |scope, args| {
                let value = args.get_owned::<Value>(scope, 0)?;

                Ok(async move {
                    tokio::task::yield_now().await;

                    Ok(Deferred::new(move |scope| {
                        value.bind::<Value>(scope)?;

                        Ok(value)
                    }))
                })
            });
        }
    }

    #[tokio::test]
    async fn carries_a_value_across_an_await() {
        assert!(
            Runtime::builder()
                .bind(DeferredHost)
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "deferred.js",
                    "import { carry } from \"@host/deferred\";\n\
                    export async function carryValue() {\n\
                        const argument = {};\n\
                        return (await carry(argument)) === argument;\n\
                    }",
                )
                .await
                .unwrap()
                .function("carryValue")
                .await
                .unwrap()
                .call::<_, Promise<bool>>(())
                .await
                .unwrap()
                .await
                .unwrap()
        );
    }
}
