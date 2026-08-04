//! Transform stream interop.

use std::{cell::RefCell, future::Future, marker::PhantomData, pin::Pin, rc::Rc};

use bytes::Bytes;
use guestjs_core::{
    errors::Error,
    handle::Instance,
    host::{args::Args, callable::HostFn},
    marshal::{FromGuest, ToGuest},
    runtime::Scope,
};
use rquickjs::{
    CatchResultExt, Constructor as JsConstructor, Ctx, Exception, Function as JsFunction,
    Object as JsObject, Value,
    function::{Async, This},
};

use crate::streams::{ReadableStream, WritableStream};

type TransformFuture<O> = Pin<Box<dyn Future<Output = Result<Vec<O>, Error>>>>;
type TransformFn<I, O> = Box<dyn FnMut(I) -> TransformFuture<O>>;

/// An owned guest transform stream.
pub struct TransformStream<I = Bytes, O = Bytes> {
    object: Instance,
    _chunks: PhantomData<fn(I) -> O>,
}

impl<I, O> TransformStream<I, O> {
    fn new(object: Instance) -> Self {
        Self { object, _chunks: PhantomData }
    }

    /// Returns the writable side.
    pub async fn writable(&self) -> Result<WritableStream<I>, Error> {
        Ok(WritableStream::new(
            self.object
                .get::<Instance>("writable")
                .await?,
        ))
    }

    /// Returns the readable side.
    pub async fn readable(&self) -> Result<ReadableStream<O>, Error> {
        Ok(ReadableStream::new(
            self.object
                .get::<Instance>("readable")
                .await?,
        ))
    }
}

impl<I, O> FromGuest for TransformStream<I, O>
where
    I: 'static,
    O: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(Instance::from_guest(scope, value)?))
    }
}

impl<I, O> ToGuest for TransformStream<I, O> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.object.to_guest(scope)
    }
}

impl<I, O> ToGuest for &TransformStream<I, O> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.object.clone().to_guest(scope)
    }
}

/// A host transform exposed as a guest `TransformStream`.
pub struct HostTransformStream<I = Bytes, O = Bytes> {
    transform: TransformFn<I, O>,
}

impl<I, O> HostTransformStream<I, O> {
    /// Creates a host transform from an asynchronous mapping.
    pub fn from_fn<F, Fut>(mut transform: F) -> Self
    where
        F: FnMut(I) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<O>, Error>> + 'static,
    {
        Self {
            transform: Box::new(move |chunk| Box::pin(transform(chunk))),
        }
    }
}

impl<I, O> ToGuest for HostTransformStream<I, O>
where
    I: FromGuest<Owned = I> + 'static,
    O: ToGuest + 'static,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        let transform = Rc::new(RefCell::new(self.transform));
        let underlying = JsObject::new(scope.ctx().clone()).catch(scope.ctx())?;

        underlying
            .set(
                "transform",
                JsFunction::new(
                    scope.ctx().clone(),
                    Async(move |ctx: Ctx<'js>, chunk: Value<'js>, controller: JsObject<'js>| {
                        let prepared = I::from_guest(&Scope::detached(ctx.clone()), chunk)
                            .map(|chunk| transform.borrow_mut()(chunk));
                        let future_ctx = ctx.clone();

                        async move {
                            let outputs = match prepared {
                                Ok(future) => future.await,
                                Err(error) => Err(error),
                            }
                            .map_err(|error| {
                                Exception::throw_message(&future_ctx, &error.to_string())
                            })?;
                            let enqueue = controller.get::<_, JsFunction>("enqueue")?;

                            for output in outputs {
                                enqueue.call::<_, ()>((
                                    This(controller.clone()),
                                    output
                                        .to_guest(&Scope::detached(future_ctx.clone()))
                                        .map_err(|error| {
                                            Exception::throw_message(
                                                &future_ctx,
                                                &error.to_string(),
                                            )
                                        })?,
                                ))?;
                            }

                            Ok::<(), rquickjs::Error>(())
                        }
                    }),
                )
                .catch(scope.ctx())?,
            )
            .catch(scope.ctx())?;

        underlying
            .set(
                "flush",
                HostFn::new(|_scope: &Scope<'_>, _args: Args<'_>| Ok(())).to_guest(scope)?,
            )
            .catch(scope.ctx())?;

        Ok(scope
            .ctx()
            .globals()
            .get::<_, JsConstructor>("TransformStream")
            .catch(scope.ctx())?
            .construct::<_, JsObject>((underlying,))
            .catch(scope.ctx())?
            .into_value())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::future::try_join;
    use guestjs_core::{handle::Promise, runtime::Runtime};

    use crate::{
        Llrt,
        streams::{HostTransformStream, TransformStream},
    };

    const GUEST_TRANSFORM_SOURCE: &str = r#"
        export const transform = new TransformStream({
            transform(chunk, controller) {
                controller.enqueue(
                    new Uint8Array(Array.from(chunk, (byte) => byte + 1)),
                );
            },
        });
    "#;

    const HOST_TRANSFORM_SOURCE: &str = r#"
        export async function apply(transform) {
            const source = new ReadableStream({
                start(controller) {
                    controller.enqueue(new Uint8Array([1, 2, 3]));
                    controller.close();
                },
            });
            const reader = source
                .pipeThrough(transform)
                .getReader();
            const bytes = [];

            while (true) {
                const { value, done } = await reader.read();

                if (done) {
                    break;
                }

                bytes.push(...value);
            }

            return bytes.join(",");
        }
    "#;

    #[tokio::test]
    async fn guest_transform_bridged_by_host() {
        let transform = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .bind_native(Llrt::builder().streams().build())
            .build()
            .await
            .unwrap()
            .guest_module("guest-transform.js", GUEST_TRANSFORM_SOURCE)
            .await
            .unwrap()
            .get::<TransformStream>("transform")
            .await
            .unwrap();
        let writer = transform
            .writable()
            .await
            .unwrap()
            .writer()
            .await
            .unwrap();
        let reader = transform.readable().await.unwrap();

        let (_, chunks) = try_join(
            async move {
                writer
                    .write(Bytes::from_static(&[1, 2, 3]))
                    .await?;
                writer.close().await
            },
            reader.collect(),
        )
        .await
        .unwrap();

        assert_eq!(chunks, vec![Bytes::from_static(&[2, 3, 4])]);
    }

    #[tokio::test]
    async fn host_transform_applied_in_guest() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().streams().build())
                .build()
                .await
                .unwrap()
                .guest_module("host-transform.js", HOST_TRANSFORM_SOURCE)
                .await
                .unwrap()
                .function("apply")
                .await
                .unwrap()
                .call::<_, Promise<String>>((HostTransformStream::from_fn(
                    |chunk: Bytes| async move {
                        Ok(vec![Bytes::from(
                            chunk
                                .iter()
                                .map(|byte| byte * 2)
                                .collect::<Vec<_>>(),
                        )])
                    },
                ),))
                .await
                .unwrap()
                .await
                .unwrap(),
            "2,4,6",
        );
    }
}
