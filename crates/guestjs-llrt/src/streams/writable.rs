//! Writable stream interop.

use std::{
    cell::RefCell,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
};

use bytes::Bytes;
use futures::{Sink, SinkExt};
use guestjs_core::{
    errors::Error,
    handle::{BoundInstance, Instance, Promise},
    host::{args::Args, callable::HostFn},
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::Scope,
};
use rquickjs::{CatchResultExt, Constructor as JsConstructor, Object as JsObject, Value as JsValue};

type ChunkSink<T> = Pin<Box<dyn Sink<T, Error = Error>>>;
type WriterFuture = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

/// A host sink exposed as a guest `WritableStream`.
pub struct HostWritableStream<T = Bytes> {
    sink: ChunkSink<T>,
}

impl<T> HostWritableStream<T> {
    /// Creates a host writable stream from a chunk sink.
    pub fn from_sink<K>(sink: K) -> Self
    where
        K: Sink<T, Error = Error> + 'static,
    {
        Self { sink: Box::pin(sink) }
    }
}

impl<T> ToGuest for HostWritableStream<T>
where
    T: FromGuest<Owned = T> + 'static,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        let sink = Rc::new(RefCell::new(Some(self.sink)));
        let underlying = JsObject::new(scope.ctx().clone()).catch(scope.ctx())?;

        underlying
            .set(
                "write",
                HostFn::new_async({
                    let sink = sink.clone();

                    move |scope: &Scope<'_>, args: Args<'_>| {
                        let chunk = args.get_owned::<T>(scope, 0)?;
                        let mut writer = sink
                            .borrow_mut()
                            .take()
                            .ok_or_else(|| Error::unexpected("write after the sink was closed"))?;
                        let sink = sink.clone();

                        Ok(async move {
                            writer.send(chunk).await?;
                            *sink.borrow_mut() = Some(writer);

                            Ok(())
                        })
                    }
                })
                .to_guest(scope)?,
            )
            .catch(scope.ctx())?;

        underlying
            .set(
                "close",
                HostFn::new_async({
                    let sink = sink.clone();

                    move |_scope: &Scope<'_>, _args: Args<'_>| {
                        let sink = sink.borrow_mut().take();

                        Ok(async move {
                            if let Some(mut sink) = sink {
                                sink.close().await?;
                            }

                            Ok(())
                        })
                    }
                })
                .to_guest(scope)?,
            )
            .catch(scope.ctx())?;

        underlying
            .set(
                "abort",
                HostFn::new({
                    let sink = sink.clone();

                    move |_scope: &Scope<'_>, _args: Args<'_>| {
                        sink.borrow_mut().take();

                        Ok(())
                    }
                })
                .to_guest(scope)?,
            )
            .catch(scope.ctx())?;

        Ok(scope
            .ctx()
            .globals()
            .get::<_, JsConstructor>("WritableStream")
            .catch(scope.ctx())?
            .construct::<_, JsObject>((underlying,))
            .catch(scope.ctx())?
            .into_value())
    }
}

impl<'js, T> ToGuestBound<'js> for HostWritableStream<T>
where
    T: FromGuest<Owned = T> + 'static,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

/// An owned guest writable stream.
pub struct WritableStream<T = Bytes> {
    object: Instance,
    _chunk: PhantomData<fn(T)>,
}

impl<T> WritableStream<T> {
    pub(crate) fn new(object: Instance) -> Self {
        Self { object, _chunk: PhantomData }
    }

    /// Binds the writable stream to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundWritableStream<'js, T>, Error> {
        Ok(BoundWritableStream {
            object: self.object.bind(scope)?,
            _chunk: PhantomData,
        })
    }

    /// Acquires a writer.
    pub async fn writer(&self) -> Result<Writer<T>, Error> {
        Ok(Writer {
            writer: self
                .object
                .call::<_, Instance>("getWriter", ())
                .await?,
            pending: None,
            closing: false,
            _chunk: PhantomData,
        })
    }

    /// Writes a chunk.
    pub async fn write(&self, chunk: T) -> Result<(), Error>
    where
        T: ToGuest + 'static,
    {
        let writer = self.writer().await?;

        writer.write(chunk).await?;
        writer.release().await
    }

    /// Closes the writable stream.
    pub async fn close(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("close", ())
            .await?
            .await
    }

    /// Aborts the writable stream.
    pub async fn abort(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("abort", ())
            .await?
            .await
    }
}

impl<T> FromGuest for WritableStream<T>
where
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(Instance::from_guest(scope, value)?))
    }
}

impl<T> FromGuestBound for WritableStream<T> {
    type Bound<'js> = BoundWritableStream<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundWritableStream {
            object: Instance::from_guest_bound(scope, value)?,
            _chunk: PhantomData,
        })
    }
}

impl<T> ToGuest for WritableStream<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.object.to_guest(scope)
    }
}

impl<T> ToGuest for &WritableStream<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.object.clone().to_guest(scope)
    }
}

impl<'js, T> ToGuestBound<'js> for WritableStream<T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

/// A guest writable stream bound to a scope.
pub struct BoundWritableStream<'js, T = Bytes> {
    object: BoundInstance<'js>,
    _chunk: PhantomData<fn(T)>,
}

impl<'js, T> BoundWritableStream<'js, T> {
    /// Acquires a writer.
    pub fn writer(&self) -> Result<BoundWriter<'js, T>, Error> {
        Ok(BoundWriter {
            writer: self
                .object
                .call::<_, Instance>("getWriter", ())?,
            _chunk: PhantomData,
        })
    }

    /// Writes a chunk.
    pub async fn write(&self, chunk: T) -> Result<(), Error>
    where
        T: ToGuestBound<'js>,
    {
        let writer = self.writer()?;

        writer.write(chunk).await?;
        writer.release()
    }

    /// Closes the writable stream.
    pub async fn close(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("close", ())?
            .await
    }

    /// Aborts the writable stream.
    pub async fn abort(&self) -> Result<(), Error> {
        self.object
            .call::<_, Promise<()>>("abort", ())?
            .await
    }

    /// Converts the writable stream into an owned handle.
    pub fn into_owned(self) -> Result<WritableStream<T>, Error> {
        Ok(WritableStream {
            object: self.object.into_owned()?,
            _chunk: PhantomData,
        })
    }
}

impl<'js, T> ToGuestBound<'js> for BoundWritableStream<'js, T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.object.to_guest_bound(scope)
    }
}

/// An owned writer for a guest writable stream.
pub struct Writer<T = Bytes> {
    writer: Instance,
    pending: Option<WriterFuture>,
    closing: bool,
    _chunk: PhantomData<fn(T)>,
}

impl<T> Writer<T> {
    fn poll_pending(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Result<(), Error>> {
        match self.pending.as_mut() {
            Some(future) => match future.as_mut().poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => {
                    self.pending = None;

                    Poll::Ready(result)
                }
            },
            None => Poll::Ready(Ok(())),
        }
    }

    /// Writes a chunk.
    pub async fn write(&self, chunk: T) -> Result<(), Error>
    where
        T: ToGuest + 'static,
    {
        self.writer
            .call::<_, Promise<()>>("write", (chunk,))
            .await?
            .await
    }

    /// Closes the writer.
    pub async fn close(&self) -> Result<(), Error> {
        self.writer
            .call::<_, Promise<()>>("close", ())
            .await?
            .await
    }

    /// Aborts the writer.
    pub async fn abort(&self) -> Result<(), Error> {
        self.writer
            .call::<_, Promise<()>>("abort", ())
            .await?
            .await
    }

    /// Releases the writer lock.
    pub async fn release(&self) -> Result<(), Error> {
        self.writer
            .call::<_, ()>("releaseLock", ())
            .await
    }
}

impl<T> Sink<T> for Writer<T>
where
    T: ToGuest + 'static,
{
    type Error = Error;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        if self.closing {
            return Poll::Ready(Err(Error::unexpected("writer is closing")));
        }

        self.as_mut().poll_pending(context)
    }

    fn start_send(mut self: Pin<&mut Self>, chunk: T) -> Result<(), Self::Error> {
        if self.pending.is_some() {
            return Err(Error::unexpected("writer is not ready"));
        }

        let writer = self.writer.clone();

        self.pending = Some(Box::pin(async move {
            writer
                .call::<_, Promise<()>>("write", (chunk,))
                .await?
                .await
        }));

        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.poll_pending(context)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().poll_pending(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }

        if self.closing {
            return Poll::Ready(Ok(()));
        }

        self.closing = true;

        let writer = self.writer.clone();

        self.pending = Some(Box::pin(async move {
            writer
                .call::<_, Promise<()>>("close", ())
                .await?
                .await
        }));

        self.poll_pending(context)
    }
}

/// A writer for a guest writable stream bound to a scope.
pub struct BoundWriter<'js, T = Bytes> {
    writer: BoundInstance<'js>,
    _chunk: PhantomData<fn(T)>,
}

impl<'js, T> BoundWriter<'js, T> {
    /// Writes a chunk.
    pub async fn write(&self, chunk: T) -> Result<(), Error>
    where
        T: ToGuestBound<'js>,
    {
        self.writer
            .call::<_, Promise<()>>("write", (chunk,))?
            .await
    }

    /// Closes the writer.
    pub async fn close(&self) -> Result<(), Error> {
        self.writer
            .call::<_, Promise<()>>("close", ())?
            .await
    }

    /// Aborts the writer.
    pub async fn abort(&self) -> Result<(), Error> {
        self.writer
            .call::<_, Promise<()>>("abort", ())?
            .await
    }

    /// Releases the writer lock.
    pub fn release(&self) -> Result<(), Error> {
        self.writer
            .call::<_, ()>("releaseLock", ())
    }

    /// Converts the writer into an owned handle.
    pub fn into_owned(self) -> Result<Writer<T>, Error> {
        Ok(Writer {
            writer: self.writer.into_owned()?,
            pending: None,
            closing: false,
            _chunk: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use bytes::Bytes;
    use futures::{SinkExt, StreamExt, channel::mpsc};
    use guestjs_core::{
        errors::Error,
        handle::Promise,
        host::{Exports, HostModule},
        runtime::Runtime,
    };

    use crate::{
        Llrt,
        streams::{HostWritableStream, WritableStream},
    };

    const HOST_WRITABLE_SOURCE: &str = r#"
        import { body } from "@host/writable";

        export async function run() {
            const writer = (await body()).getWriter();

            await writer.write(new Uint8Array([1, 2]));
            await writer.write(new Uint8Array([3]));
            await writer.close();
        }
    "#;

    const GUEST_WRITABLE_SOURCE: &str = r#"
        const chunks = [];

        export function target() {
            return new WritableStream({
                write(chunk) {
                    chunks.push(Array.from(chunk));
                },
            });
        }

        export function received() {
            return chunks
                .map((chunk) => chunk.join(","))
                .join("|");
        }
    "#;

    struct WritableHost {
        sender: RefCell<Option<mpsc::UnboundedSender<Bytes>>>,
    }

    impl HostModule for WritableHost {
        fn name(&self) -> &str {
            "@host/writable"
        }

        fn build(&self, exports: &mut Exports) {
            let sender = RefCell::new(self.sender.borrow_mut().take());

            exports.async_function("body", move |_scope, _args| {
                let sender = sender
                    .borrow_mut()
                    .take()
                    .ok_or_else(|| Error::unexpected("writable body was already requested"))?;

                Ok(async move {
                    Ok(HostWritableStream::from_sink(
                        sender.sink_map_err(|error| Error::unexpected(error.to_string())),
                    ))
                })
            });
        }
    }

    #[tokio::test]
    async fn host_writable_receives_guest_writes() {
        let (sender, receiver) = mpsc::unbounded::<Bytes>();

        Runtime::builder()
            .bind(WritableHost { sender: RefCell::new(Some(sender)) })
            .build()
            .await
            .unwrap()
            .guest()
            .bind_native(Llrt::builder().streams().build())
            .build()
            .await
            .unwrap()
            .guest_module("host-writable.js", HOST_WRITABLE_SOURCE)
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

        assert_eq!(
            receiver.collect::<Vec<_>>().await,
            vec![Bytes::from_static(&[1, 2]), Bytes::from_static(&[3])],
        );
    }

    #[tokio::test]
    async fn guest_writable_written_by_host() {
        let module = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .bind_native(Llrt::builder().streams().build())
            .build()
            .await
            .unwrap()
            .guest_module("guest-writable.js", GUEST_WRITABLE_SOURCE)
            .await
            .unwrap();
        let explicit = module
            .function("target")
            .await
            .unwrap()
            .call::<_, WritableStream>(())
            .await
            .unwrap()
            .writer()
            .await
            .unwrap();

        explicit
            .write(Bytes::from_static(&[1, 2]))
            .await
            .unwrap();
        explicit
            .write(Bytes::from_static(&[3]))
            .await
            .unwrap();
        explicit.close().await.unwrap();

        let mut sink = module
            .function("target")
            .await
            .unwrap()
            .call::<_, WritableStream>(())
            .await
            .unwrap()
            .writer()
            .await
            .unwrap();

        SinkExt::send(&mut sink, Bytes::from_static(&[4]))
            .await
            .unwrap();
        SinkExt::send(&mut sink, Bytes::from_static(&[5, 6]))
            .await
            .unwrap();
        SinkExt::close(&mut sink).await.unwrap();

        assert_eq!(
            module
                .function("received")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2|3|4|5,6",
        );
    }
}
