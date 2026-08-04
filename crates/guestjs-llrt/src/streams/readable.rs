//! Readable stream interop.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use guestjs_core::{
    errors::Error,
    handle::{BoundInstance, Instance, Promise},
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::Scope,
};
use llrt_modules::stream_web::{
    CancelAlgorithm as LlrtCancelAlgorithm, PullAlgorithm as LlrtPullAlgorithm,
    ReadableStream as LlrtReadableStream, ReadableStreamControllerClass as LlrtControllerClass,
    readable_stream_default_controller_close_stream,
    readable_stream_default_controller_enqueue_value,
    readable_stream_default_controller_error_stream,
};
use rquickjs::{CatchResultExt, Ctx, Exception, Promise as JsPromise, Value};

use crate::streams::{TransformStream, WritableStream};

type ChunkSource<T> = Pin<Box<dyn Stream<Item = Result<T, Error>>>>;

/// A host source exposed as a guest `ReadableStream`.
pub struct HostReadableStream<T = Bytes> {
    source: ChunkSource<T>,
}

impl<T> HostReadableStream<T> {
    /// Creates a host readable stream from a chunk source.
    pub fn from_stream<S>(source: S) -> Self
    where
        S: Stream<Item = Result<T, Error>> + 'static,
    {
        Self { source: Box::pin(source) }
    }

    async fn pull_once<'js>(
        ctx: Ctx<'js>,
        controller: LlrtControllerClass<'js>,
        source: Rc<RefCell<Option<ChunkSource<T>>>>,
        cancelled: Rc<Cell<bool>>,
    ) -> rquickjs::Result<()>
    where
        T: ToGuest + 'static,
    {
        let LlrtControllerClass::ReadableStreamDefaultController(controller) = controller else {
            return Err(Exception::throw_type(&ctx, "expected a default controller"));
        };

        if cancelled.get() {
            return readable_stream_default_controller_close_stream(ctx, controller);
        }

        let Some(mut stream) = source.borrow_mut().take() else {
            return readable_stream_default_controller_close_stream(ctx, controller);
        };

        match stream.next().await {
            _ if cancelled.get() => {
                readable_stream_default_controller_close_stream(ctx, controller)
            }
            Some(Ok(chunk)) => {
                *source.borrow_mut() = Some(stream);

                readable_stream_default_controller_enqueue_value(
                    ctx.clone(),
                    controller,
                    chunk
                        .to_guest(&Scope::detached(ctx.clone()))
                        .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))?,
                )
            }
            Some(Err(error)) => readable_stream_default_controller_error_stream(
                controller,
                Exception::from_message(ctx, &error.to_string())?.into_value(),
            ),
            None => readable_stream_default_controller_close_stream(ctx, controller),
        }
    }
}

impl<T> ToGuest for HostReadableStream<T>
where
    T: ToGuest + 'static,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        let source = Rc::new(RefCell::new(Some(self.source)));
        let cancelled = Rc::new(Cell::new(false));

        Ok(LlrtReadableStream::from_pull_algorithm(
            scope.ctx().clone(),
            LlrtPullAlgorithm::from_fn({
                let source = source.clone();
                let cancelled = cancelled.clone();

                move |ctx, controller| {
                    let source = source.clone();
                    let cancelled = cancelled.clone();
                    let future_ctx = ctx.clone();

                    JsPromise::wrap_future(&ctx, async move {
                        Self::pull_once(future_ctx, controller, source, cancelled).await
                    })
                }
            }),
            LlrtCancelAlgorithm::from_fn({
                let source = source.clone();

                move |reason| {
                    cancelled.set(true);
                    *source.borrow_mut() = None;

                    JsPromise::wrap_future(reason.ctx(), async {})
                }
            }),
        )
        .catch(scope.ctx())?
        .into_value())
    }
}

impl<'js, T> ToGuestBound<'js> for HostReadableStream<T>
where
    T: ToGuest + 'static,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

struct ReadOutcome<T> {
    _chunk: PhantomData<fn() -> T>,
}

impl<T> FromGuest for ReadOutcome<T>
where
    T: FromGuest,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        let result = value
            .into_object()
            .ok_or_else(|| Error::conversion("read() result is not an object"))?;

        if result
            .get::<_, bool>("done")
            .catch(scope.ctx())?
        {
            return Ok(None);
        }

        Ok(Some(T::from_guest(
            scope,
            result
                .get::<_, Value>("value")
                .catch(scope.ctx())?,
        )?))
    }
}

impl<T> FromGuestBound for ReadOutcome<T>
where
    T: FromGuestBound,
{
    type Bound<'js> = Option<T::Bound<'js>>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        let result = value
            .into_object()
            .ok_or_else(|| Error::conversion("read() result is not an object"))?;

        if result
            .get::<_, bool>("done")
            .catch(scope.ctx())?
        {
            return Ok(None);
        }

        Ok(Some(T::from_guest_bound(
            scope,
            result
                .get::<_, Value>("value")
                .catch(scope.ctx())?,
        )?))
    }
}

/// An owned guest readable stream.
pub struct ReadableStream<T = Bytes> {
    object: Instance,
    _chunk: PhantomData<fn() -> T>,
}

impl<T> ReadableStream<T> {
    pub(crate) fn new(object: Instance) -> Self {
        Self { object, _chunk: PhantomData }
    }
}

impl<T> ReadableStream<T>
where
    T: FromGuest + 'static,
{
    /// Binds the readable stream to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundReadableStream<'js, T>, Error> {
        Ok(BoundReadableStream {
            object: self.object.bind(scope)?,
            _chunk: PhantomData,
        })
    }

    /// Acquires a reader.
    pub async fn reader(&self) -> Result<Reader<T>, Error> {
        Ok(Reader {
            reader: self
                .object
                .call::<_, Instance>("getReader", ())
                .await?,
            pending: None,
            _chunk: PhantomData,
        })
    }

    /// Collects the remaining chunks.
    pub async fn collect(&self) -> Result<Vec<T::Owned>, Error> {
        let reader = self.reader().await?;
        let mut chunks = Vec::new();

        while let Some(chunk) = reader.read().await? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    /// Cancels the readable stream.
    pub async fn cancel(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("cancel", ())
            .await?
            .await
    }

    /// Pipes the readable stream through a transform.
    pub async fn pipe_through<O>(
        &self,
        transform: &TransformStream<T, O>,
    ) -> Result<ReadableStream<O>, Error>
    where
        O: 'static,
    {
        self.object
            .call::<_, ReadableStream<O>>("pipeThrough", (transform,))
            .await
    }

    /// Pipes the readable stream into a writable stream.
    pub async fn pipe_to(&self, destination: &WritableStream<T>) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("pipeTo", (destination,))
            .await?
            .await
    }

    /// Splits the readable stream into two branches.
    pub async fn tee(&self) -> Result<(ReadableStream<T>, ReadableStream<T>), Error> {
        let mut branches = self
            .object
            .call::<_, Vec<ReadableStream<T>>>("tee", ())
            .await?;

        if branches.len() != 2 {
            return Err(Error::conversion("tee() did not return two streams"));
        }

        Ok((branches.remove(0), branches.remove(0)))
    }
}

impl<T> FromGuest for ReadableStream<T>
where
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(Instance::from_guest(scope, value)?))
    }
}

impl<T> FromGuestBound for ReadableStream<T> {
    type Bound<'js> = BoundReadableStream<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundReadableStream {
            object: Instance::from_guest_bound(scope, value)?,
            _chunk: PhantomData,
        })
    }
}

impl<T> ToGuest for ReadableStream<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.object.to_guest(scope)
    }
}

impl<'js, T> ToGuestBound<'js> for ReadableStream<T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

/// A guest readable stream bound to a scope.
pub struct BoundReadableStream<'js, T = Bytes> {
    object: BoundInstance<'js>,
    _chunk: PhantomData<fn() -> T>,
}

impl<'js, T> BoundReadableStream<'js, T>
where
    T: FromGuestBound + 'js,
{
    /// Acquires a reader.
    pub fn reader(&self) -> Result<BoundReader<'js, T>, Error> {
        Ok(BoundReader {
            reader: self
                .object
                .call::<_, Instance>("getReader", ())?,
            _chunk: PhantomData,
        })
    }

    /// Collects the remaining chunks.
    pub async fn collect(&self) -> Result<Vec<T::Bound<'js>>, Error> {
        let reader = self.reader()?;
        let mut chunks = Vec::new();

        while let Some(chunk) = reader.read().await? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    /// Cancels the readable stream.
    pub async fn cancel(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("cancel", ())?
            .await
    }

    /// Converts the readable stream into an owned handle.
    pub fn into_owned(self) -> Result<ReadableStream<T>, Error> {
        Ok(ReadableStream {
            object: self.object.into_owned()?,
            _chunk: PhantomData,
        })
    }
}

impl<'js, T> ToGuestBound<'js> for BoundReadableStream<'js, T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.object.to_guest_bound(scope)
    }
}

type PendingReadFuture<T> = Pin<Box<dyn Future<Output = Result<Option<T>, Error>>>>;

/// An owned reader for a guest readable stream.
pub struct Reader<T = Bytes>
where
    T: FromGuest,
{
    reader: Instance,
    pending: Option<PendingReadFuture<T::Owned>>,
    _chunk: PhantomData<fn() -> T>,
}

impl<T> Reader<T>
where
    T: FromGuest + 'static,
{
    /// Reads the next chunk.
    pub async fn read(&self) -> Result<Option<T::Owned>, Error> {
        self.reader
            .call::<_, Promise<ReadOutcome<T>>>("read", ())
            .await?
            .await
    }

    /// Releases the reader lock.
    pub async fn release(&self) -> Result<(), Error> {
        self.reader
            .call::<_, ()>("releaseLock", ())
            .await
    }
}

impl<T> Stream for Reader<T>
where
    T: FromGuest + 'static,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.pending.is_none() {
            let reader = self.reader.clone();

            self.pending = Some(Box::pin(async move {
                reader
                    .call::<_, Promise<ReadOutcome<T>>>("read", ())
                    .await?
                    .await
            }));
        }

        match self
            .pending
            .as_mut()
            .unwrap()
            .as_mut()
            .poll(context)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(None)) => {
                self.pending = None;

                Poll::Ready(None)
            }
            Poll::Ready(Ok(Some(chunk))) => {
                self.pending = None;

                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Err(error)) => {
                self.pending = None;

                Poll::Ready(Some(Err(error)))
            }
        }
    }
}

/// A reader for a guest readable stream bound to a scope.
pub struct BoundReader<'js, T = Bytes> {
    reader: BoundInstance<'js>,
    _chunk: PhantomData<fn() -> T>,
}

impl<'js, T> BoundReader<'js, T>
where
    T: FromGuestBound + 'js,
{
    /// Reads the next chunk.
    pub async fn read(&self) -> Result<Option<T::Bound<'js>>, Error> {
        self.reader
            .call::<_, Promise<ReadOutcome<T>>>("read", ())?
            .await
    }

    /// Releases the reader lock.
    pub fn release(&self) -> Result<(), Error> {
        self.reader
            .call::<_, ()>("releaseLock", ())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };

    use bytes::Bytes;
    use futures::{Stream, TryStreamExt, future::try_join, stream};
    use guestjs_core::{
        errors::Error,
        handle::Promise,
        host::{Exports, HostModule},
        runtime::Runtime,
    };

    use crate::{
        Llrt,
        streams::{HostReadableStream, ReadableStream},
    };

    const HOST_READABLE_SOURCE: &str = r#"
        import { Service } from "@host/service";

        export async function run() {
            const stream = await new Service().body();
            const bytes = [];
            let chunks = 0;

            for await (const chunk of stream) {
                chunks += 1;
                bytes.push(...chunk);
            }

            return `${chunks}:${bytes.join(",")}`;
        }
    "#;

    const CANCEL_READABLE_SOURCE: &str = r#"
        import { body } from "@host/cancellation";

        export async function run() {
            const reader = body().getReader();

            await reader.cancel();
        }
    "#;

    const GUEST_READABLE_SOURCE: &str = r#"
        export function body() {
            return new ReadableStream({
                start(controller) {
                    controller.enqueue(new Uint8Array([1, 2]));
                    controller.enqueue(new Uint8Array([3]));
                    controller.close();
                },
            });
        }
    "#;

    struct Service;

    #[guestjs_macros::host_class(
        crate_path = guestjs_core,
        rename_all = "camelCase",
    )]
    impl Service {
        #[guestjs(constructor)]
        fn new() -> Result<Self, Error> {
            Ok(Self)
        }

        #[guestjs(async_method)]
        fn body(
            &self,
        ) -> Result<impl Future<Output = Result<HostReadableStream, Error>> + 'static, Error>
        {
            Ok(async move {
                Ok(HostReadableStream::from_stream(stream::iter([
                    Ok::<_, Error>(Bytes::from_static(b"foo")),
                    Ok(Bytes::from_static(b"bar")),
                    Ok(Bytes::from_static(b"baz")),
                ])))
            })
        }
    }

    struct ServiceHost;

    #[guestjs_macros::host_module(
        crate_path = guestjs_core,
        name = "@host/service",
        classes(Service),
    )]
    impl ServiceHost {}

    struct TrackedSource {
        dropped: Rc<Cell<bool>>,
    }

    impl Drop for TrackedSource {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }

    impl Stream for TrackedSource {
        type Item = Result<Bytes, Error>;

        fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(Some(Ok(Bytes::from_static(b"chunk"))))
        }
    }

    struct CancellationHost {
        dropped: Rc<Cell<bool>>,
    }

    impl HostModule for CancellationHost {
        fn name(&self) -> &str {
            "@host/cancellation"
        }

        fn build(&self, exports: &mut Exports) {
            let dropped = self.dropped.clone();

            exports.function("body", move |_scope, _args| {
                Ok(HostReadableStream::from_stream(TrackedSource { dropped: dropped.clone() }))
            });
        }
    }

    #[tokio::test]
    async fn host_readable_streams_bytes_to_guest() {
        assert_eq!(
            Runtime::builder()
                .bind(ServiceHost)
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().streams().build())
                .build()
                .await
                .unwrap()
                .guest_module("consume.js", HOST_READABLE_SOURCE)
                .await
                .unwrap()
                .function("run")
                .await
                .unwrap()
                .call::<_, Promise<String>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            "3:102,111,111,98,97,114,98,97,122",
        );
    }

    #[tokio::test]
    async fn host_readable_cancel_drops_source() {
        let dropped = Rc::new(Cell::new(false));

        Runtime::builder()
            .bind(CancellationHost { dropped: dropped.clone() })
            .build()
            .await
            .unwrap()
            .guest()
            .bind_native(Llrt::builder().streams().build())
            .build()
            .await
            .unwrap()
            .guest_module("cancel.js", CANCEL_READABLE_SOURCE)
            .await
            .unwrap()
            .function("run")
            .await
            .unwrap()
            .call::<_, Promise<()>>(())
            .await
            .unwrap()
            .await
            .unwrap();

        assert!(dropped.get());
    }

    #[tokio::test]
    async fn guest_readable_stream_drained_by_host() {
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
                .guest_module("produce.js", GUEST_READABLE_SOURCE)
                .await
                .unwrap()
                .function("body")
                .await
                .unwrap()
                .call::<_, ReadableStream>(())
                .await
                .unwrap()
                .collect()
                .await
                .unwrap(),
            vec![Bytes::from_static(&[1, 2]), Bytes::from_static(&[3])],
        );
    }

    #[tokio::test]
    async fn guest_readable_stream_as_futures_stream() {
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
                .guest_module("stream-produce.js", GUEST_READABLE_SOURCE)
                .await
                .unwrap()
                .function("body")
                .await
                .unwrap()
                .call::<_, ReadableStream>(())
                .await
                .unwrap()
                .reader()
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![Bytes::from_static(&[1, 2]), Bytes::from_static(&[3])],
        );
    }

    #[tokio::test]
    async fn readable_tee_produces_two_branches() {
        let stream = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .bind_native(Llrt::builder().streams().build())
            .build()
            .await
            .unwrap()
            .guest_module("tee.js", GUEST_READABLE_SOURCE)
            .await
            .unwrap()
            .function("body")
            .await
            .unwrap()
            .call::<_, ReadableStream>(())
            .await
            .unwrap();
        let (left, right) = stream.tee().await.unwrap();
        let (left, right) = try_join(left.collect(), right.collect())
            .await
            .unwrap();

        assert_eq!(left, vec![Bytes::from_static(&[1, 2]), Bytes::from_static(&[3])],);
        assert_eq!(right, left);
    }
}
