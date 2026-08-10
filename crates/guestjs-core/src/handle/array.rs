use std::{io, marker::PhantomData, mem, ptr, rc::Rc};

use rquickjs::{
    CatchResultExt, Persistent, TypedArray as JsTypedArray, Value as JsValue,
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
        Some(unsafe { (raw.ptr.as_ptr() as *const T).add(index).read_unaligned() })
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
            (raw.ptr.as_ptr() as *mut T).add(index).write_unaligned(value);
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

impl<'js> BoundTypedArray<'js, u8> {
    fn detached() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "the guest buffer is detached")
    }
}

impl<'js> io::Write for BoundTypedArray<'js, u8> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let raw = self.value.as_raw().ok_or_else(Self::detached)?;
        let count = buf.len().min(raw.len.saturating_sub(self.position));

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
        let raw = self.value.as_raw().ok_or_else(Self::detached)?;
        let count = buf.len().min(raw.len.saturating_sub(self.position));

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
        let len = self.value.as_raw().ok_or_else(Self::detached)?.len;

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

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom, Write};

    use crate::{
        handle::{Float64Array, Uint8Array},
        host::{Exports, HostModule},
        runtime::Runtime,
    };

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
                     readBytes,\n\
                     readZeroLengthView,\n\
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
                 }",
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn reads_and_writes_typed_elements() {
        let module = array_module().await;
        assert!(module
            .function("readsAndWritesTypedElements")
            .await
            .unwrap()
            .call::<_, bool>(())
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn writes_into_a_guest_uint8array() {
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
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
        let module = array_module().await;
        assert_eq!(
            module
                .function("aZeroLengthViewReadsNothing")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "0,0",
        );
    }
}
