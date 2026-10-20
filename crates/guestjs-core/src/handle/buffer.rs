use std::marker::PhantomData;

use crate::{
    backend::{
        Backend, Val,
        buffers::BackendBuffers,
        values::{BackendValues, Kind},
    },
    enter::Enter,
    errors::Error,
    handle::{
        HasHandle,
        base::Handle,
        protocol::{BufferProtocol, ObjectProtocol, ViewProtocol},
    },
    marshal::{
        FromGuest, ToGuest,
        buffers::Element,
        describe::{Describe, Expected},
    },
};

pub struct ArrayBuffer<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for ArrayBuffer<B> {
    fn clone(&self) -> Self {
        Self { handle: self.handle.clone() }
    }
}

impl<B: Backend> HasHandle<B> for ArrayBuffer<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for ArrayBuffer<B> {}

impl<B: BackendBuffers> BufferProtocol<B> for ArrayBuffer<B> {}

impl<B: Backend> Describe for ArrayBuffer<B> {
    fn describe(expected: &mut Expected) {
        expected.push("ArrayBuffer");
    }
}

impl<B: BackendValues> FromGuest<B> for ArrayBuffer<B> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::ArrayBuffer => Ok(Self { handle: Handle::from_value(enter, value) }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for ArrayBuffer<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &ArrayBuffer<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

pub struct TypedArray<B: Backend, T> {
    handle: Handle<B>,
    element: PhantomData<T>,
}

impl<B: BackendBuffers, T: Element> TypedArray<B, T> {
    pub fn from_values<A>(enter: &Enter<'_, B>, values: A) -> Result<Self, Error>
    where
        A: AsRef<[T]>,
    {
        Self::from_guest(enter, values.as_ref().to_guest(enter)?)
    }

    pub fn len(&self) -> Result<usize, Error> {
        self.byte_length()
            .map(|byte_length| byte_length / T::TYPE.size())
    }

    pub fn is_empty(&self) -> Result<bool, Error> {
        self.len().map(|len| len == 0)
    }

    pub fn to_vec(&self) -> Result<Vec<T>, Error> {
        self.bytes()
            .map(|bytes| T::from_bytes(&bytes))
    }

    pub fn write(&self, index: usize, values: &[T]) -> Result<(), Error> {
        self.write_bytes(index.saturating_mul(T::TYPE.size()), &T::to_bytes(values))
    }
}

impl<B: Backend, T> Clone for TypedArray<B, T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            element: PhantomData,
        }
    }
}

impl<B: Backend, T> HasHandle<B> for TypedArray<B, T> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues, T> ObjectProtocol<B> for TypedArray<B, T> {}

impl<B: BackendBuffers, T> BufferProtocol<B> for TypedArray<B, T> {}

impl<B: BackendBuffers, T> ViewProtocol<B> for TypedArray<B, T> {}

impl<B: Backend, T: Element> Describe for TypedArray<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push(T::TYPE.name());
    }
}

impl<B: BackendBuffers, T: Element> FromGuest<B> for TypedArray<B, T> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::TypedArray => match B::element_type(enter.token(), &value)? {
                element if element == T::TYPE => Ok(Self {
                    handle: Handle::from_value(enter, value),
                    element: PhantomData,
                }),
                element => {
                    Err(Error::conversion(Expected::of::<Self>().to_string(), element.name()))
                }
            },
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for TypedArray<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &TypedArray<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

pub type Int8Array<B> = TypedArray<B, i8>;
pub type Uint8Array<B> = TypedArray<B, u8>;
pub type Int16Array<B> = TypedArray<B, i16>;
pub type Uint16Array<B> = TypedArray<B, u16>;
pub type Int32Array<B> = TypedArray<B, i32>;
pub type Uint32Array<B> = TypedArray<B, u32>;
pub type BigInt64Array<B> = TypedArray<B, i64>;
pub type BigUint64Array<B> = TypedArray<B, u64>;
pub type Float32Array<B> = TypedArray<B, f32>;
pub type Float64Array<B> = TypedArray<B, f64>;

pub struct DataView<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for DataView<B> {
    fn clone(&self) -> Self {
        Self { handle: self.handle.clone() }
    }
}

impl<B: Backend> HasHandle<B> for DataView<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for DataView<B> {}

impl<B: BackendBuffers> BufferProtocol<B> for DataView<B> {}

impl<B: BackendBuffers> ViewProtocol<B> for DataView<B> {}

impl<B: Backend> Describe for DataView<B> {
    fn describe(expected: &mut Expected) {
        expected.push("DataView");
    }
}

impl<B: BackendValues> FromGuest<B> for DataView<B> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::DataView => Ok(Self { handle: Handle::from_value(enter, value) }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for DataView<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &DataView<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{buffers::BackendBuffers, tests::Stub},
        handle::{
            buffer::{ArrayBuffer, DataView, Float64Array, TypedArray, Uint8Array},
            protocol::{BufferProtocol, ObjectProtocol, ViewProtocol},
        },
        marshal::{FromGuest, ToGuest, describe::Expected},
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendBuffers>() {
        fn object<B: BackendBuffers, T: ObjectProtocol<B>>() {}
        fn buffer<B: BackendBuffers, T: BufferProtocol<B>>() {}
        fn view<B: BackendBuffers, T: ViewProtocol<B>>() {}
        fn from<B: BackendBuffers, T: FromGuest<B>>() {}
        fn to<B: BackendBuffers, T: ToGuest<B>>() {}

        object::<B, ArrayBuffer<B>>();
        buffer::<B, ArrayBuffer<B>>();
        from::<B, ArrayBuffer<B>>();
        to::<B, ArrayBuffer<B>>();
        to::<B, &ArrayBuffer<B>>();
        object::<B, TypedArray<B, f32>>();
        buffer::<B, TypedArray<B, f32>>();
        view::<B, TypedArray<B, f32>>();
        from::<B, TypedArray<B, f32>>();
        to::<B, TypedArray<B, f32>>();
        to::<B, &TypedArray<B, f32>>();
        object::<B, DataView<B>>();
        buffer::<B, DataView<B>>();
        view::<B, DataView<B>>();
        from::<B, DataView<B>>();
        to::<B, DataView<B>>();
        to::<B, &DataView<B>>();
    }

    #[test]
    fn descriptions_name_the_javascript_types() {
        assert_eq!(Expected::of::<ArrayBuffer<Stub>>().to_string(), "ArrayBuffer");
        assert_eq!(Expected::of::<Uint8Array<Stub>>().to_string(), "Uint8Array");
        assert_eq!(Expected::of::<Float64Array<Stub>>().to_string(), "Float64Array");
        assert_eq!(Expected::of::<DataView<Stub>>().to_string(), "DataView");
    }
}
