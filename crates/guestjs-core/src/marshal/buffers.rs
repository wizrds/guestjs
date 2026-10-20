#[cfg(feature = "bytes")]
use bytes::Bytes;

use crate::{
    backend::{
        buffers::{
            BackendBuffers,
            ElementType,
        },
        values::Kind,
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
    },
};

pub trait Element: Copy + 'static {
    const TYPE: ElementType;

    fn from_bytes(bytes: &[u8]) -> Vec<Self>;

    fn to_bytes(values: &[Self]) -> Vec<u8>;
}

macro_rules! element {
    ($($type:ty => $element:ident),+) => {
        $(
            impl Element for $type {
                const TYPE: ElementType = ElementType::$element;

                fn from_bytes(bytes: &[u8]) -> Vec<Self> {
                    bytes
                        .as_chunks::<{ size_of::<$type>() }>()
                        .0
                        .iter()
                        .map(|chunk| Self::from_ne_bytes(*chunk))
                        .collect()
                }

                fn to_bytes(values: &[Self]) -> Vec<u8> {
                    values
                        .iter()
                        .flat_map(|value| value.to_ne_bytes())
                        .collect()
                }
            }
        )+
    };
}

element!(
    i8 => Int8,
    u8 => Uint8,
    i16 => Int16,
    u16 => Uint16,
    i32 => Int32,
    u32 => Uint32,
    i64 => BigInt64,
    u64 => BigUint64,
    f32 => Float32,
    f64 => Float64
);

pub struct BufferSource<'js, B: Backend> {
    buffer: Val<'js, B>,
    byte_offset: usize,
    byte_length: usize,
}

impl<'js, B: BackendBuffers> BufferSource<'js, B> {
    pub fn read(
        enter: &Enter<'js, B>,
        value: &Val<'js, B>,
    ) -> Result<Self, Error> {
        match B::kind(enter.token(), value) {
            Kind::ArrayBuffer => Ok(Self {
                buffer: value.clone(),
                byte_offset: 0,
                byte_length: B::byte_length(enter.token(), value)?,
            }),
            Kind::TypedArray | Kind::DataView => {
                match B::buffer(enter.token(), value)? {
                    buffer if B::kind(enter.token(), &buffer) == Kind::ArrayBuffer => {
                        Ok(Self {
                            buffer,
                            byte_offset: B::byte_offset(enter.token(), value)?,
                            byte_length: B::byte_length(enter.token(), value)?,
                        })
                    }
                    _ => Err(Error::conversion(
                        "view of an ArrayBuffer",
                        "view of a SharedArrayBuffer",
                    )),
                }
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }

    pub fn bytes(&self, enter: &Enter<'js, B>) -> Result<Vec<u8>, Error> {
        B::read(
            enter.token(),
            &self.buffer,
            self.byte_offset,
            self.byte_length,
        )
    }

    pub fn write(
        &self,
        enter: &Enter<'js, B>,
        byte_offset: usize,
        bytes: &[u8],
    ) -> Result<(), Error> {
        match byte_offset
            .checked_add(bytes.len())
            .is_some_and(|end| end <= self.byte_length)
        {
            true => B::write(
                enter.token(),
                &self.buffer,
                self.byte_offset + byte_offset,
                bytes,
            ),
            false => Err(Error::conversion(
                format!(
                    "at most {} bytes at byte offset {byte_offset}",
                    self.byte_length.saturating_sub(byte_offset),
                ),
                format!("{} bytes", bytes.len()),
            )),
        }
    }
}

impl<B: Backend> Describe for BufferSource<'_, B> {
    fn describe(expected: &mut Expected) {
        expected.push("ArrayBuffer");
        expected.push("typed array");
        expected.push("DataView");
    }
}

impl<B, T> ToGuest<B> for &[T]
where
    B: BackendBuffers,
    T: Element,
{
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        B::new_typed_array(
            enter.token(),
            T::TYPE,
            &B::new_array_buffer(enter.token(), &T::to_bytes(self))?,
            0,
            self.len(),
        )
    }
}

#[cfg(feature = "bytes")]
impl Describe for Bytes {
    fn describe(expected: &mut Expected) {
        expected.push("ArrayBuffer");
        expected.push("typed array");
        expected.push("DataView");
    }
}

#[cfg(feature = "bytes")]
impl<B: BackendBuffers> ToGuest<B> for Bytes {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.as_ref().to_guest(enter)
    }
}

#[cfg(feature = "bytes")]
impl<B: BackendBuffers> ToGuest<B> for &Bytes {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.as_ref().to_guest(enter)
    }
}

#[cfg(feature = "bytes")]
impl<B: BackendBuffers> FromGuest<B> for Bytes {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        BufferSource::read(enter, &value)?
            .bytes(enter)
            .map(Bytes::from)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "bytes")]
    use bytes::Bytes;

    use crate::{
        backend::{
            buffers::{
                BackendBuffers,
                ElementType,
            },
            tests::Stub,
        },
        marshal::{
            buffers::{
                BufferSource,
                Element,
            },
            describe::Expected,
            FromGuest,
            ToGuest,
        },
    };

    #[allow(dead_code)]
    fn conversions<B: BackendBuffers>() {
        fn to<B: BackendBuffers, T: ToGuest<B>>() {}

        to::<B, &[u8]>();
        to::<B, &[i64]>();
        to::<B, &[f64]>();
    }

    #[cfg(feature = "bytes")]
    #[allow(dead_code)]
    fn bytes_conversions<B: BackendBuffers>() {
        fn to<B: BackendBuffers, T: ToGuest<B>>() {}
        fn from<B: BackendBuffers, T: FromGuest<B>>() {}

        to::<B, Bytes>();
        to::<B, &Bytes>();
        from::<B, Bytes>();
    }

    #[test]
    fn elements_match_their_element_type_size() {
        assert_eq!(i8::TYPE.size(), size_of::<i8>());
        assert_eq!(u8::TYPE.size(), size_of::<u8>());
        assert_eq!(i16::TYPE.size(), size_of::<i16>());
        assert_eq!(u16::TYPE.size(), size_of::<u16>());
        assert_eq!(i32::TYPE.size(), size_of::<i32>());
        assert_eq!(u32::TYPE.size(), size_of::<u32>());
        assert_eq!(i64::TYPE.size(), size_of::<i64>());
        assert_eq!(u64::TYPE.size(), size_of::<u64>());
        assert_eq!(f32::TYPE.size(), size_of::<f32>());
        assert_eq!(f64::TYPE.size(), size_of::<f64>());
    }

    #[test]
    fn sixty_four_bit_integers_are_bigint_elements() {
        assert_eq!(i64::TYPE, ElementType::BigInt64);
        assert_eq!(u64::TYPE, ElementType::BigUint64);
    }

    #[test]
    fn elements_round_trip_through_native_endian_bytes() {
        assert_eq!(i16::to_bytes(&[1]), 1i16.to_ne_bytes().to_vec());
        assert_eq!(f32::from_bytes(&f32::to_bytes(&[1.5, -2.0])), vec![1.5, -2.0]);
        assert_eq!(u64::from_bytes(&u64::to_bytes(&[u64::MAX])), vec![u64::MAX]);
    }

    #[test]
    fn buffer_sources_accept_every_binary_kind() {
        assert_eq!(
            Expected::of::<BufferSource<Stub>>().to_string(),
            "ArrayBuffer, typed array or DataView",
        );
    }

    #[cfg(feature = "bytes")]
    #[test]
    fn bytes_accept_every_binary_kind() {
        assert_eq!(
            Expected::of::<Bytes>().to_string(),
            "ArrayBuffer, typed array or DataView",
        );
    }
}
