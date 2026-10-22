use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use bytemuck::{AnyBitPattern, NoUninit};
#[cfg(feature = "bytes")]
use bytes::Bytes;

use crate::{
    backend::{
        Val,
        buffers::{BackendBuffers, ElementType},
        values::Kind,
    },
    enter::Enter,
    errors::Error,
    marshal::{
        FromGuest, FromGuestMut, FromGuestRef, ToGuest,
        describe::{Describe, Expected},
    },
};

pub trait Element: NoUninit + AnyBitPattern {
    const TYPE: ElementType;

    fn check(bytes: &[u8]) -> Result<(), Error> {
        match bytes.is_empty() {
            true => Ok(()),
            false => bytemuck::try_cast_slice::<u8, Self>(bytes)
                .map(|_| ())
                .map_err(|_| {
                    Error::conversion(
                        format!(
                            "{}-byte aligned bytes in multiples of {}",
                            align_of::<Self>(),
                            size_of::<Self>(),
                        ),
                        format!("{} bytes", bytes.len()),
                    )
                }),
        }
    }

    fn cast(bytes: &[u8]) -> &[Self] {
        match bytes.is_empty() {
            true => &[],
            false => bytemuck::cast_slice(bytes),
        }
    }

    fn cast_mut(bytes: &mut [u8]) -> &mut [Self] {
        match bytes.is_empty() {
            true => &mut [],
            false => bytemuck::cast_slice_mut(bytes),
        }
    }

    fn as_bytes(values: &[Self]) -> &[u8] {
        bytemuck::cast_slice(values)
    }
}

macro_rules! element {
    ($($type:ty => $element:ident),+) => {
        $(
            impl Element for $type {
                const TYPE: ElementType = ElementType::$element;
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

pub struct SliceRef<'a, B: BackendBuffers, T> {
    bytes: B::Bytes<'a>,
    element: PhantomData<T>,
}

impl<B: BackendBuffers, T: Element> Deref for SliceRef<'_, B, T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        T::cast(&self.bytes)
    }
}

pub struct SliceMut<'a, B: BackendBuffers, T> {
    bytes: B::BytesMut<'a>,
    element: PhantomData<T>,
}

impl<B: BackendBuffers, T: Element> Deref for SliceMut<'_, B, T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        T::cast(&self.bytes)
    }
}

impl<B: BackendBuffers, T: Element> DerefMut for SliceMut<'_, B, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        T::cast_mut(&mut self.bytes)
    }
}

impl<T: Element> Describe for [T] {
    fn describe(expected: &mut Expected) {
        expected.push("ArrayBuffer");
        expected.push("typed array");
        expected.push("DataView");
    }
}

impl<B: BackendBuffers, T: Element> FromGuestRef<B> for [T] {
    type Ref<'a> = SliceRef<'a, B, T>;

    fn from_guest_ref<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Ref<'a>, Error> {
        match B::kind(enter.token(), value) {
            Kind::ArrayBuffer | Kind::TypedArray | Kind::DataView => {
                B::borrow_bytes(enter.token(), value).and_then(|bytes| {
                    T::check(&bytes).map(|_| SliceRef { bytes, element: PhantomData })
                })
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: BackendBuffers, T: Element> FromGuestMut<B> for [T] {
    type Mut<'a> = SliceMut<'a, B, T>;

    fn from_guest_mut<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Mut<'a>, Error> {
        match B::kind(enter.token(), value) {
            Kind::ArrayBuffer | Kind::TypedArray | Kind::DataView => {
                B::borrow_bytes_mut(enter.token(), value).and_then(|bytes| {
                    T::check(&bytes).map(|_| SliceMut { bytes, element: PhantomData })
                })
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B, T> ToGuest<B> for &[T]
where
    B: BackendBuffers,
    T: Element,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        B::new_typed_array(
            enter.token(),
            T::TYPE,
            &B::new_array_buffer(enter.token(), T::as_bytes(self))?,
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
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.as_ref().to_guest(enter)
    }
}

#[cfg(feature = "bytes")]
impl<B: BackendBuffers> ToGuest<B> for &Bytes {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.as_ref().to_guest(enter)
    }
}

#[cfg(feature = "bytes")]
impl<B: BackendBuffers> FromGuest<B> for Bytes {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        <[u8] as FromGuestRef<B>>::from_guest_ref(enter, &value)
            .map(|bytes| Bytes::copy_from_slice(&bytes))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "bytes")]
    use bytes::Bytes;

    use crate::{
        backend::buffers::{BackendBuffers, ElementType},
        marshal::{
            FromGuest, FromGuestMut, FromGuestRef, ToGuest, buffers::Element, describe::Expected,
        },
    };

    #[allow(dead_code)]
    fn conversions<B: BackendBuffers>() {
        fn to<B: BackendBuffers, T: ToGuest<B>>() {}
        fn from_ref<B: BackendBuffers, T: FromGuestRef<B> + ?Sized>() {}
        fn from_mut<B: BackendBuffers, T: FromGuestMut<B> + ?Sized>() {}

        to::<B, &[u8]>();
        to::<B, &[i64]>();
        to::<B, &[f64]>();
        from_ref::<B, [u8]>();
        from_mut::<B, [f64]>();
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
    fn elements_check_whole_aligned_ranges() {
        let words = [0u64; 2];
        let bytes = u64::as_bytes(&words);

        u64::check(bytes).unwrap();
        u64::check(&bytes[8..]).unwrap();
        u64::check(&bytes[..0]).unwrap();

        assert!(u64::check(&bytes[1..9]).is_err());
        assert!(u64::check(&bytes[..12]).is_err());
    }

    #[test]
    fn elements_cast_without_copying() {
        let values = [1.5f32, -2.0];
        let bytes = f32::as_bytes(&values);

        assert_eq!(bytes.as_ptr(), values.as_ptr().cast());
        assert_eq!(f32::cast(bytes), &values);
    }

    #[test]
    fn empty_bytes_cast_to_empty_elements() {
        assert!(u64::cast(&[]).is_empty());
    }

    #[test]
    fn slices_accept_every_binary_kind() {
        assert_eq!(Expected::of::<[u8]>().to_string(), "ArrayBuffer, typed array or DataView");
    }

    #[cfg(feature = "bytes")]
    #[test]
    fn bytes_accept_every_binary_kind() {
        assert_eq!(Expected::of::<Bytes>().to_string(), "ArrayBuffer, typed array or DataView",);
    }
}
