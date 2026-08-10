use std::{io, marker::PhantomData, mem, ptr, rc::Rc};

#[cfg(feature = "bytes")]
use bytes::{Buf, BufMut, buf::UninitSlice};
use rquickjs::{
    Array as JsArray, ArrayBuffer as JsArrayBuffer, CatchResultExt, Persistent,
    TypedArray as JsTypedArray, Value as JsValue,
};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::{GuestContext, Scope},
};

mod sealed {
    pub trait Sealed {}
}

pub trait TypedArrayElement: sealed::Sealed + Copy + 'static {
    fn from_guest_value<'js>(value: JsValue<'js>) -> rquickjs::Result<JsTypedArray<'js, Self>>;
}

macro_rules! typed_array_elements {
    ($($element:ty),* $(,)?) => {
        $(
            impl sealed::Sealed for $element {}

            impl TypedArrayElement for $element {
                fn from_guest_value<'js>(
                    value: JsValue<'js>,
                ) -> rquickjs::Result<JsTypedArray<'js, Self>> {
                    JsTypedArray::<$element>::from_value(value)
                }
            }
        )*
    };
}

typed_array_elements!(i8, u8, i16, u16, i32, u32, f32, f64, i64, u64);

pub struct TypedArray<T> {
    value: Persistent<JsValue<'static>>,
    context: Rc<GuestContext>,
    element: PhantomData<T>,
}

impl<T> Clone for TypedArray<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            context: self.context.clone(),
            element: PhantomData,
        }
    }
}

impl<T> TypedArray<T>
where
    T: TypedArrayElement,
{
    pub(crate) fn new(value: Persistent<JsValue<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context, element: PhantomData }
    }

    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundTypedArray<'js, T>, Error> {
        Ok(BoundTypedArray::new(
            T::from_guest_value(
                self.value
                    .clone()
                    .restore(scope.ctx())
                    .catch(scope.ctx())?,
            )
            .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    pub async fn len(&self) -> Result<usize, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.len())).await
    }

    pub async fn is_empty(&self) -> Result<bool, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.is_empty())).await
    }

    pub async fn get(&self, index: usize) -> Result<Option<T>, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.get(index))).await
    }

    pub async fn set(&self, index: usize, value: T) -> Result<bool, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.set(index, value)))
            .await
    }
}

impl<T> ToGuest for TypedArray<T>
where
    T: TypedArrayElement,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.value
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js, T> ToGuestBound<'js> for TypedArray<T>
where
    T: TypedArrayElement,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl<T> FromGuest for TypedArray<T>
where
    T: TypedArrayElement,
{
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(TypedArray::new(
            Persistent::save(
                scope.ctx(),
                T::from_guest_value(value)
                    .catch(scope.ctx())?
                    .into_value(),
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<T> FromGuestBound for TypedArray<T>
where
    T: TypedArrayElement,
{
    type Bound<'js> = BoundTypedArray<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundTypedArray::new(
            T::from_guest_value(value).catch(scope.ctx())?,
            scope.clone(),
        ))
    }
}

pub struct BoundTypedArray<'js, T> {
    value: JsTypedArray<'js, T>,
    scope: Scope<'js>,
    position: usize,
}

impl<'js, T> BoundTypedArray<'js, T>
where
    T: TypedArrayElement,
{
    pub(crate) fn new(value: JsTypedArray<'js, T>, scope: Scope<'js>) -> Self {
        Self { value, scope, position: 0 }
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<T> {
        let raw = self.value.as_raw()?;

        if index >= raw.len / mem::size_of::<T>() {
            return None;
        }

        // SAFETY: no JavaScript runs between `as_raw` and the read, so the pointer stays valid,
        // and the bounds check above keeps the read inside the allocation. The engine gives no
        // alignment guarantee, hence `read_unaligned`.
        Some(unsafe {
            (raw.ptr.as_ptr() as *const T)
                .add(index)
                .read_unaligned()
        })
    }

    pub fn set(&self, index: usize, value: T) -> bool {
        let Some(raw) = self.value.as_raw() else {
            return false;
        };

        if index >= raw.len / mem::size_of::<T>() {
            return false;
        }

        // SAFETY: as in `get`, with the bounds check keeping the write inside the allocation.
        unsafe {
            (raw.ptr.as_ptr() as *mut T)
                .add(index)
                .write_unaligned(value);
        }

        true
    }

    pub fn into_owned(self) -> Result<TypedArray<T>, Error> {
        Ok(TypedArray::new(
            Persistent::save(self.scope.ctx(), self.value.into_value()),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js, T> ToGuestBound<'js> for BoundTypedArray<'js, T> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self.value.into_value())
    }
}

impl<'js> io::Write for BoundTypedArray<'js, u8> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let raw = self.value.as_raw().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
        })?;
        let count = buf
            .len()
            .min(raw.len.saturating_sub(self.position));

        // SAFETY: no JavaScript runs between `as_raw` and the copy, so the pointer stays valid,
        // and `count` is clamped to the bytes left after `position`. Host and engine memory
        // cannot overlap.
        unsafe {
            ptr::copy_nonoverlapping(buf.as_ptr(), raw.ptr.as_ptr().add(self.position), count);
        }

        self.position += count;

        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'js> io::Read for BoundTypedArray<'js, u8> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let raw = self.value.as_raw().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
        })?;
        let count = buf
            .len()
            .min(raw.len.saturating_sub(self.position));

        // SAFETY: as in the `Write` impl above, with the copy running the other direction.
        unsafe {
            ptr::copy_nonoverlapping(raw.ptr.as_ptr().add(self.position), buf.as_mut_ptr(), count);
        }

        self.position += count;

        Ok(count)
    }
}

impl<'js> io::Seek for BoundTypedArray<'js, u8> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let len = self
            .value
            .as_raw()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
            })?
            .len;

        let target = match pos {
            io::SeekFrom::Start(offset) => offset as i128,
            io::SeekFrom::End(offset) => len as i128 + offset as i128,
            io::SeekFrom::Current(offset) => self.position as i128 + offset as i128,
        };

        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the guest buffer",
            ));
        }

        self.position = target as usize;

        Ok(self.position as u64)
    }
}

#[cfg(feature = "bytes")]
impl<'js> Buf for BoundTypedArray<'js, u8> {
    fn remaining(&self) -> usize {
        self.value
            .as_raw()
            .map_or(0, |raw| raw.len.saturating_sub(self.position))
    }

    fn chunk(&self) -> &[u8] {
        self.value
            .as_bytes()
            .and_then(|bytes| bytes.get(self.position..))
            .unwrap_or_default()
    }

    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.remaining());

        self.position += cnt;
    }
}

#[cfg(feature = "bytes")]
unsafe impl<'js> BufMut for BoundTypedArray<'js, u8> {
    fn remaining_mut(&self) -> usize {
        self.value
            .as_raw()
            .map_or(0, |raw| raw.len.saturating_sub(self.position))
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        self.position += cnt;
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        match self.value.as_raw() {
            // SAFETY: no JavaScript runs between `as_raw` and the wrap, so the pointer stays valid,
            // and the range from `position` to `raw.len` is inside the allocation. The returned
            // borrow cannot outlive this `&mut self`.
            Some(raw) if self.position < raw.len => unsafe {
                UninitSlice::from_raw_parts_mut(
                    raw.ptr.as_ptr().add(self.position),
                    raw.len - self.position,
                )
            },
            // SAFETY: a dangling but well-aligned pointer with a length of zero is a valid empty
            // slice.
            _ => unsafe {
                UninitSlice::from_raw_parts_mut(ptr::NonNull::<u8>::dangling().as_ptr(), 0)
            },
        }
    }
}

pub type Int8Array = TypedArray<i8>;
pub type Uint8Array = TypedArray<u8>;
pub type Int16Array = TypedArray<i16>;
pub type Uint16Array = TypedArray<u16>;
pub type Int32Array = TypedArray<i32>;
pub type Uint32Array = TypedArray<u32>;
pub type Float32Array = TypedArray<f32>;
pub type Float64Array = TypedArray<f64>;
pub type BigInt64Array = TypedArray<i64>;
pub type BigUint64Array = TypedArray<u64>;

pub type BoundInt8Array<'js> = BoundTypedArray<'js, i8>;
pub type BoundUint8Array<'js> = BoundTypedArray<'js, u8>;
pub type BoundInt16Array<'js> = BoundTypedArray<'js, i16>;
pub type BoundUint16Array<'js> = BoundTypedArray<'js, u16>;
pub type BoundInt32Array<'js> = BoundTypedArray<'js, i32>;
pub type BoundUint32Array<'js> = BoundTypedArray<'js, u32>;
pub type BoundFloat32Array<'js> = BoundTypedArray<'js, f32>;
pub type BoundFloat64Array<'js> = BoundTypedArray<'js, f64>;
pub type BoundBigInt64Array<'js> = BoundTypedArray<'js, i64>;
pub type BoundBigUint64Array<'js> = BoundTypedArray<'js, u64>;

#[derive(Clone)]
pub struct ArrayBuffer {
    value: Persistent<JsArrayBuffer<'static>>,
    context: Rc<GuestContext>,
}

impl ArrayBuffer {
    pub(crate) fn new(
        value: Persistent<JsArrayBuffer<'static>>,
        context: Rc<GuestContext>,
    ) -> Self {
        Self { value, context }
    }

    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundArrayBuffer<'js>, Error> {
        Ok(BoundArrayBuffer::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    pub async fn len(&self) -> Result<usize, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.len())).await
    }

    pub async fn is_empty(&self) -> Result<bool, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.is_empty())).await
    }
}

impl ToGuest for ArrayBuffer {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self
            .value
            .restore(scope.ctx())
            .catch(scope.ctx())?
            .into_value())
    }
}

impl<'js> ToGuestBound<'js> for ArrayBuffer {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for ArrayBuffer {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(ArrayBuffer::new(
            Persistent::save(
                scope.ctx(),
                JsArrayBuffer::from_value(value)
                    .ok_or_else(|| Error::conversion("expected an array buffer"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl FromGuestBound for ArrayBuffer {
    type Bound<'js> = BoundArrayBuffer<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundArrayBuffer::new(
            JsArrayBuffer::from_value(value)
                .ok_or_else(|| Error::conversion("expected an array buffer"))?,
            scope.clone(),
        ))
    }
}

pub struct BoundArrayBuffer<'js> {
    value: JsArrayBuffer<'js>,
    scope: Scope<'js>,
    position: usize,
}

impl<'js> BoundArrayBuffer<'js> {
    pub(crate) fn new(value: JsArrayBuffer<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope, position: 0 }
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn into_owned(self) -> Result<ArrayBuffer, Error> {
        Ok(ArrayBuffer::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js> ToGuestBound<'js> for BoundArrayBuffer<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self.value.into_value())
    }
}

impl<'js> io::Write for BoundArrayBuffer<'js> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let raw = self.value.as_raw().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
        })?;
        let count = buf
            .len()
            .min(raw.len.saturating_sub(self.position));

        // SAFETY: no JavaScript runs between `as_raw` and the copy, so the pointer stays valid,
        // and `count` is clamped to the bytes left after `position`. Host and engine memory
        // cannot overlap.
        unsafe {
            ptr::copy_nonoverlapping(buf.as_ptr(), raw.ptr.as_ptr().add(self.position), count);
        }

        self.position += count;

        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'js> io::Read for BoundArrayBuffer<'js> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let raw = self.value.as_raw().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
        })?;
        let count = buf
            .len()
            .min(raw.len.saturating_sub(self.position));

        // SAFETY: as in the `Write` impl above, with the copy running the other direction.
        unsafe {
            ptr::copy_nonoverlapping(raw.ptr.as_ptr().add(self.position), buf.as_mut_ptr(), count);
        }

        self.position += count;

        Ok(count)
    }
}

impl<'js> io::Seek for BoundArrayBuffer<'js> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let len = self
            .value
            .as_raw()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
            })?
            .len;

        let target = match pos {
            io::SeekFrom::Start(offset) => offset as i128,
            io::SeekFrom::End(offset) => len as i128 + offset as i128,
            io::SeekFrom::Current(offset) => self.position as i128 + offset as i128,
        };

        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the guest buffer",
            ));
        }

        self.position = target as usize;

        Ok(self.position as u64)
    }
}

#[cfg(feature = "bytes")]
impl<'js> Buf for BoundArrayBuffer<'js> {
    fn remaining(&self) -> usize {
        self.value
            .as_raw()
            .map_or(0, |raw| raw.len.saturating_sub(self.position))
    }

    fn chunk(&self) -> &[u8] {
        self.value
            .as_bytes()
            .and_then(|bytes| bytes.get(self.position..))
            .unwrap_or_default()
    }

    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.remaining());

        self.position += cnt;
    }
}

#[cfg(feature = "bytes")]
unsafe impl<'js> BufMut for BoundArrayBuffer<'js> {
    fn remaining_mut(&self) -> usize {
        self.value
            .as_raw()
            .map_or(0, |raw| raw.len.saturating_sub(self.position))
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        self.position += cnt;
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        match self.value.as_raw() {
            // SAFETY: as in the `BoundTypedArray` impl above.
            Some(raw) if self.position < raw.len => unsafe {
                UninitSlice::from_raw_parts_mut(
                    raw.ptr.as_ptr().add(self.position),
                    raw.len - self.position,
                )
            },
            // SAFETY: a dangling but well-aligned pointer with a length of zero is a valid empty
            // slice.
            _ => unsafe {
                UninitSlice::from_raw_parts_mut(ptr::NonNull::<u8>::dangling().as_ptr(), 0)
            },
        }
    }
}

#[derive(Clone)]
pub struct Array {
    value: Persistent<JsArray<'static>>,
    context: Rc<GuestContext>,
}

impl Array {
    pub(crate) fn new(value: Persistent<JsArray<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context }
    }

    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundArray<'js>, Error> {
        Ok(BoundArray::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    pub async fn len(&self) -> Result<usize, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.len())).await
    }

    pub async fn is_empty(&self) -> Result<bool, Error> {
        Scope::with(&self.context, async move |scope| Ok(self.bind(&scope)?.is_empty())).await
    }

    pub async fn get<R>(&self, index: usize) -> Result<R::Owned, Error>
    where
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(&scope, self.bind(&scope)?.get_value(index)?)
        })
        .await
    }

    pub async fn set<V>(&self, index: usize, value: V) -> Result<(), Error>
    where
        V: ToGuest,
    {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .set_value(index, value.to_guest(&scope)?)
        })
        .await
    }
}

impl ToGuest for Array {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self
            .value
            .restore(scope.ctx())
            .catch(scope.ctx())?
            .into_value())
    }
}

impl<'js> ToGuestBound<'js> for Array {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Array {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Array::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_array()
                    .ok_or_else(|| Error::conversion("expected an array"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl FromGuestBound for Array {
    type Bound<'js> = BoundArray<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundArray::new(
            value
                .into_array()
                .ok_or_else(|| Error::conversion("expected an array"))?,
            scope.clone(),
        ))
    }
}

pub struct BoundArray<'js> {
    value: JsArray<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundArray<'js> {
    pub(crate) fn new(value: JsArray<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    fn get_value(&self, index: usize) -> Result<JsValue<'js>, Error> {
        self.value
            .get::<JsValue>(index)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn set_value(&self, index: usize, value: JsValue<'js>) -> Result<(), Error> {
        self.value
            .set(index, value)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn get<R>(&self, index: usize) -> Result<R::Bound<'js>, Error>
    where
        R: FromGuestBound,
    {
        R::from_guest_bound(&self.scope, self.get_value(index)?)
    }

    pub fn set<V>(&self, index: usize, value: V) -> Result<(), Error>
    where
        V: ToGuestBound<'js>,
    {
        self.set_value(index, value.to_guest_bound(&self.scope)?)
    }

    pub fn into_owned(self) -> Result<Array, Error> {
        Ok(Array::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js> ToGuestBound<'js> for BoundArray<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self.value.into_value())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom, Write};

    use rquickjs::{CatchResultExt, Value as JsValue};

    use crate::{
        handle::{Array, ArrayBuffer, Float64Array, Promise, Scoped, Uint8Array, Value},
        host::{Exports, HostModule},
        marshal::FromGuestBound,
        runtime::{Runtime, Scope},
    };

    struct BufferModule;

    impl HostModule for BufferModule {
        fn name(&self) -> &str {
            "test:buffers"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("fillSync", |scope, args| {
                args.get::<Uint8Array>(scope, 0)?
                    .write_all(&[1, 2, 3, 4])?;

                Ok(())
            });
            exports.async_function("fillAsync", |scope, args| {
                let value = args.get_owned::<Value>(scope, 0)?;

                Ok(async move {
                    tokio::task::yield_now().await;

                    Ok(Scoped::new(move |scope: &Scope| {
                        value
                            .bind::<Uint8Array>(scope)?
                            .write_all(&[1, 2, 3, 4])?;

                        Ok(value)
                    }))
                })
            });
        }
    }

    struct ArrayHost;

    impl HostModule for ArrayHost {
        fn name(&self) -> &str {
            "@host/array"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("getAndSetFloat", |scope, args| {
                let array = args.get::<Float64Array>(scope, 0)?;
                array.set(1, 2.5);

                Ok(array.get(1))
            });
            exports.function("writeBytes", |scope, args| {
                args.get::<Uint8Array>(scope, 0)?
                    .write_all(&[1, 2, 3, 4])?;

                Ok(())
            });
            exports.function("writeBytesAfterSeek", |scope, args| {
                let mut array = args.get::<Uint8Array>(scope, 0)?;
                array.seek(SeekFrom::Start(2))?;
                array.write_all(&[9, 9])?;

                Ok(())
            });
            exports.function("readBytes", |scope, args| {
                let mut array = args.get::<Uint8Array>(scope, 0)?;
                let mut buffer = [0u8; 4];
                array.read_exact(&mut buffer)?;

                Ok(buffer.to_vec())
            });
            exports.function("readAfterSeek", |scope, args| {
                let mut array = args.get::<Uint8Array>(scope, 0)?;
                array.seek(SeekFrom::Start(2))?;
                let mut buffer = [0u8; 8];

                Ok(array.read(&mut buffer)?)
            });
            exports.function("partialThenZeroWrite", |scope, args| {
                let mut array = args.get::<Uint8Array>(scope, 0)?;
                let first = array.write(&[1, 2, 3, 4])?;
                let second = array.write(&[1, 2, 3, 4])?;

                Ok(vec![first, second])
            });
            exports.function("readZeroLengthView", |scope, args| {
                let mut array = args.get::<Uint8Array>(scope, 0)?;
                let mut buffer = [0u8; 1];

                Ok(vec![array.len(), array.read(&mut buffer)?])
            });
            exports.function("writeBufferBytes", |scope, args| {
                args.get::<ArrayBuffer>(scope, 0)?
                    .write_all(&[1, 2, 3, 4])?;

                Ok(())
            });
            exports.function("readBufferBytes", |scope, args| {
                let mut buffer = args.get::<ArrayBuffer>(scope, 0)?;
                let mut bytes = [0u8; 4];
                buffer.read_exact(&mut bytes)?;

                Ok(bytes.to_vec())
            });
        }
    }

    async fn array_module() -> crate::handle::Module {
        Runtime::builder()
            .bind(ArrayHost)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "array.js",
                "import {\n\
                     getAndSetFloat,\n\
                     partialThenZeroWrite,\n\
                     readAfterSeek,\n\
                     readBufferBytes,\n\
                     readBytes,\n\
                     readZeroLengthView,\n\
                     writeBufferBytes,\n\
                     writeBytes,\n\
                     writeBytesAfterSeek,\n\
                 } from \"@host/array\";\n\
                 export function readsAndWritesTypedElements() {\n\
                     const array = new Float64Array(3);\n\
                     const result = getAndSetFloat(array);\n\
                     return result === 2.5 && array[1] === 2.5;\n\
                 }\n\
                 export function writesIntoAGuestUint8Array() {\n\
                     const buffer = new Uint8Array(4);\n\
                     writeBytes(buffer);\n\
                     return Array.from(buffer).join(\",\");\n\
                 }\n\
                 export function writesAfterASeekLeaveEarlierBytesUntouched() {\n\
                     const buffer = new Uint8Array(4);\n\
                     writeBytesAfterSeek(buffer);\n\
                     return Array.from(buffer).join(\",\");\n\
                 }\n\
                 export function writesRespectAViewOffset() {\n\
                     const bytes = new Uint8Array(8);\n\
                     const view = new Uint8Array(bytes.buffer, 4, 4);\n\
                     writeBytes(view);\n\
                     return Array.from(bytes).join(\",\");\n\
                 }\n\
                 export function readsOutOfAGuestUint8Array() {\n\
                     const buffer = Uint8Array.from([1, 2, 3, 4]);\n\
                     return readBytes(buffer).join(\",\");\n\
                 }\n\
                 export function readsAreClampedToTheRemainingBytes() {\n\
                     const buffer = new Uint8Array(4);\n\
                     return readAfterSeek(buffer);\n\
                 }\n\
                 export function aWritePastTheEndIsPartialThenZero() {\n\
                     const buffer = new Uint8Array(2);\n\
                     return partialThenZeroWrite(buffer).join(\",\");\n\
                 }\n\
                 export function aZeroLengthViewReadsNothing() {\n\
                     return readZeroLengthView(new Uint8Array(0)).join(\",\");\n\
                 }\n\
                 export function writesIntoABareArrayBuffer() {\n\
                     const buffer = new ArrayBuffer(4);\n\
                     writeBufferBytes(buffer);\n\
                     return Array.from(new Uint8Array(buffer)).join(\",\");\n\
                 }\n\
                 export function readsOutOfABareArrayBuffer() {\n\
                     return readBufferBytes(Uint8Array.from([5, 6, 7, 8]).buffer).join(\",\");\n\
                 }\n\
                 export function aNonBufferIsRejected() {\n\
                     try {\n\
                         readBufferBytes(\"not a buffer\");\n\
                         return \"no error\";\n\
                     } catch (error) {\n\
                         return String(error);\n\
                     }\n\
                 }",
            )
            .await
            .unwrap()
    }

    async fn buffer_module() -> crate::handle::Module {
        Runtime::builder()
            .bind(BufferModule)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "buffers.js",
                "import { fillAsync, fillSync } from \"test:buffers\";\n\
                 export function sync() {\n\
                     const buffer = new Uint8Array(4);\n\
                     fillSync(buffer);\n\
                     return Array.from(buffer).join(\",\");\n\
                 }\n\
                 export async function asynchronous() {\n\
                     const buffer = new Uint8Array(4);\n\
                     const result = await fillAsync(buffer);\n\
                     return [Array.from(buffer).join(\",\"), String(result === buffer)].join(\";\");\n\
                 }",
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn reads_and_writes_typed_elements() {
        assert!(
            array_module()
                .await
                .function("readsAndWritesTypedElements")
                .await
                .unwrap()
                .call::<_, bool>(())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn writes_into_a_guest_uint8array() {
        assert_eq!(
            array_module()
                .await
                .function("writesIntoAGuestUint8Array")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn writes_after_a_seek_leave_earlier_bytes_untouched() {
        assert_eq!(
            array_module()
                .await
                .function("writesAfterASeekLeaveEarlierBytesUntouched")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "0,0,9,9",
        );
    }

    #[tokio::test]
    async fn writes_respect_a_view_offset() {
        assert_eq!(
            array_module()
                .await
                .function("writesRespectAViewOffset")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "0,0,0,0,1,2,3,4",
        );
    }

    #[tokio::test]
    async fn reads_out_of_a_guest_uint8array() {
        assert_eq!(
            array_module()
                .await
                .function("readsOutOfAGuestUint8Array")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn reads_are_clamped_to_the_remaining_bytes() {
        assert_eq!(
            array_module()
                .await
                .function("readsAreClampedToTheRemainingBytes")
                .await
                .unwrap()
                .call::<_, usize>(())
                .await
                .unwrap(),
            2,
        );
    }

    #[tokio::test]
    async fn a_write_past_the_end_is_partial_then_zero() {
        assert_eq!(
            array_module()
                .await
                .function("aWritePastTheEndIsPartialThenZero")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "2,0",
        );
    }

    #[tokio::test]
    async fn a_zero_length_view_reads_nothing() {
        assert_eq!(
            array_module()
                .await
                .function("aZeroLengthViewReadsNothing")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "0,0",
        );
    }

    #[tokio::test]
    async fn writes_into_a_bare_array_buffer() {
        assert_eq!(
            array_module()
                .await
                .function("writesIntoABareArrayBuffer")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn reads_out_of_a_bare_array_buffer() {
        assert_eq!(
            array_module()
                .await
                .function("readsOutOfABareArrayBuffer")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "5,6,7,8",
        );
    }

    #[tokio::test]
    async fn reads_and_writes_array_elements_in_a_scope() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async move |scope| {
                let array = Array::from_guest_bound(
                    &scope,
                    scope
                        .ctx()
                        .eval::<JsValue, _>("[10, 20, 30]")
                        .catch(scope.ctx())?,
                )?;

                assert_eq!(array.len(), 3);
                assert_eq!(array.get::<i32>(1)?, 20);

                array.set(1, 99)?;

                assert_eq!(array.get::<i32>(1)?, 99);

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn array_survives_across_scopes() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let array = guest
            .eval::<Array>("[1, 2, 3]")
            .await
            .unwrap();

        guest
            .scope(async move |scope| {
                assert_eq!(array.bind(&scope)?.get::<i32>(0)?, 1);

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reads_and_writes_array_elements_without_a_scope() {
        let array = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .eval::<Array>("[10, 20, 30]")
            .await
            .unwrap();

        assert_eq!(array.len().await.unwrap(), 3);
        assert_eq!(array.get::<i32>(1).await.unwrap(), 20);

        array.set(1, 99).await.unwrap();

        assert_eq!(array.get::<i32>(1).await.unwrap(), 99);
    }

    #[tokio::test]
    async fn reads_typed_array_elements_without_a_scope() {
        let array = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .eval::<Float64Array>("Float64Array.from([1.5, 2.5])")
            .await
            .unwrap();

        assert_eq!(array.len().await.unwrap(), 2);
        assert_eq!(array.get(1).await.unwrap(), Some(2.5));

        assert!(array.set(0, 9.5).await.unwrap());
        assert_eq!(array.get(0).await.unwrap(), Some(9.5));
    }

    #[tokio::test]
    async fn reports_an_array_buffer_length_without_a_scope() {
        let buffer = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .eval::<ArrayBuffer>("new ArrayBuffer(8)")
            .await
            .unwrap();

        assert_eq!(buffer.len().await.unwrap(), 8);
        assert!(!buffer.is_empty().await.unwrap());
    }

    #[tokio::test]
    async fn a_non_buffer_is_rejected() {
        assert!(
            array_module()
                .await
                .function("aNonBufferIsRejected")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap()
                .contains("expected")
        );
    }

    #[tokio::test]
    async fn a_synchronous_host_function_fills_a_caller_supplied_buffer() {
        assert_eq!(
            buffer_module()
                .await
                .function("sync")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn an_asynchronous_host_function_fills_a_caller_supplied_buffer_and_returns_it() {
        assert_eq!(
            buffer_module()
                .await
                .function("asynchronous")
                .await
                .unwrap()
                .call::<_, Promise<String>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            "1,2,3,4;true",
        );
    }
}

#[cfg(all(test, feature = "bytes"))]
mod bytes_tests {
    use bytes::{Buf, BufMut};

    use crate::{
        handle::{ArrayBuffer, Uint8Array},
        host::{Exports, HostModule},
        runtime::Runtime,
    };

    struct ArrayBytesHost;

    impl HostModule for ArrayBytesHost {
        fn name(&self) -> &str {
            "@host/array-bytes"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("fillBuffer", |scope, args| {
                args.get::<Uint8Array>(scope, 0)?
                    .put_slice(&[1, 2, 3, 4]);

                Ok(())
            });
            exports.function("drainBuffer", |scope, args| {
                Ok(args
                    .get::<Uint8Array>(scope, 0)?
                    .copy_to_bytes(4))
            });
            exports.function("fillArrayBuffer", |scope, args| {
                args.get::<ArrayBuffer>(scope, 0)?
                    .put_slice(&[1, 2, 3, 4]);

                Ok(())
            });
            exports.function("drainArrayBuffer", |scope, args| {
                Ok(args
                    .get::<ArrayBuffer>(scope, 0)?
                    .copy_to_bytes(4))
            });
        }
    }

    async fn bytes_module() -> crate::handle::Module {
        Runtime::builder()
            .bind(ArrayBytesHost)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "array-bytes.js",
                "import {\n\
                     drainArrayBuffer,\n\
                     drainBuffer,\n\
                     fillArrayBuffer,\n\
                     fillBuffer,\n\
                 } from \"@host/array-bytes\";\n\
                 export function fillsAGuestBufferThroughBufmut() {\n\
                     const buffer = new Uint8Array(4);\n\
                     fillBuffer(buffer);\n\
                     return Array.from(buffer).join(\",\");\n\
                 }\n\
                 export function readsAGuestBufferThroughBuf() {\n\
                     return Array.from(drainBuffer(Uint8Array.from([5, 6, 7, 8]))).join(\",\");\n\
                 }\n\
                 export function fillsAGuestArrayBufferThroughBufmut() {\n\
                     const buffer = new ArrayBuffer(4);\n\
                     fillArrayBuffer(buffer);\n\
                     return Array.from(new Uint8Array(buffer)).join(\",\");\n\
                 }\n\
                 export function readsAGuestArrayBufferThroughBuf() {\n\
                     const bytes = Uint8Array.from([5, 6, 7, 8]);\n\
                     return Array.from(drainArrayBuffer(bytes.buffer)).join(\",\");\n\
                 }",
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fills_a_guest_buffer_through_bufmut() {
        assert_eq!(
            bytes_module()
                .await
                .function("fillsAGuestBufferThroughBufmut")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn reads_a_guest_buffer_through_buf() {
        assert_eq!(
            bytes_module()
                .await
                .function("readsAGuestBufferThroughBuf")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "5,6,7,8",
        );
    }

    #[tokio::test]
    async fn fills_a_guest_array_buffer_through_bufmut() {
        assert_eq!(
            bytes_module()
                .await
                .function("fillsAGuestArrayBufferThroughBufmut")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "1,2,3,4",
        );
    }

    #[tokio::test]
    async fn reads_a_guest_array_buffer_through_buf() {
        assert_eq!(
            bytes_module()
                .await
                .function("readsAGuestArrayBufferThroughBuf")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "5,6,7,8",
        );
    }
}
