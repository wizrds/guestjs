use rquickjs::Value as JsValue;

use crate::{errors::Error, handle::value::Value, marshal::ToGuest, runtime::Scope};

pub struct Scoped<F> {
    callback: F,
}

impl<F> Scoped<F> {
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ToGuest for Scoped<F>
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
        handle::{Promise, Scoped, Value},
        host::{Exports, HostModule},
        runtime::{Runtime, Scope},
    };

    struct ScopedHost;

    impl HostModule for ScopedHost {
        fn name(&self) -> &str {
            "@host/scoped"
        }

        fn build(&self, exports: &mut Exports) {
            exports.async_function("carry", |scope, args| {
                let value = args.get_owned::<Value>(scope, 0)?;

                Ok(async move {
                    tokio::task::yield_now().await;

                    Ok(Scoped::new(move |scope: &Scope| {
                        value.bind::<Value>(scope)?;

                        Ok(value)
                    }))
                })
            });
        }
    }

    #[tokio::test]
    async fn carries_a_value_across_an_await() {
        let module = Runtime::builder()
            .bind(ScopedHost)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "scoped.js",
                "import { carry } from \"@host/scoped\";\n\
                 export async function carryValue() {\n\
                     const argument = {};\n\
                     return (await carry(argument)) === argument;\n\
                 }",
            )
            .await
            .unwrap();

        assert!(
            module
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
