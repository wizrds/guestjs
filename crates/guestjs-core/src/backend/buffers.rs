use crate::{
    backend::{Tok, Val, values::BackendValues},
    errors::Error,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ElementType {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}

impl ElementType {
    pub const ALL: [ElementType; 12] = [
        ElementType::Int8,
        ElementType::Uint8,
        ElementType::Uint8Clamped,
        ElementType::Int16,
        ElementType::Uint16,
        ElementType::Int32,
        ElementType::Uint32,
        ElementType::BigInt64,
        ElementType::BigUint64,
        ElementType::Float16,
        ElementType::Float32,
        ElementType::Float64,
    ];

    pub fn size(self) -> usize {
        match self {
            ElementType::Int8 | ElementType::Uint8 | ElementType::Uint8Clamped => 1,
            ElementType::Int16 | ElementType::Uint16 | ElementType::Float16 => 2,
            ElementType::Int32 | ElementType::Uint32 | ElementType::Float32 => 4,
            ElementType::BigInt64 | ElementType::BigUint64 | ElementType::Float64 => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ElementType::Int8 => "Int8Array",
            ElementType::Uint8 => "Uint8Array",
            ElementType::Uint8Clamped => "Uint8ClampedArray",
            ElementType::Int16 => "Int16Array",
            ElementType::Uint16 => "Uint16Array",
            ElementType::Int32 => "Int32Array",
            ElementType::Uint32 => "Uint32Array",
            ElementType::BigInt64 => "BigInt64Array",
            ElementType::BigUint64 => "BigUint64Array",
            ElementType::Float16 => "Float16Array",
            ElementType::Float32 => "Float32Array",
            ElementType::Float64 => "Float64Array",
        }
    }
}

pub trait BackendBuffers: BackendValues {
    fn new_array_buffer<'js>(token: Tok<'js, Self>, bytes: &[u8]) -> Result<Val<'js, Self>, Error>;
    fn new_typed_array<'js>(
        token: Tok<'js, Self>,
        element: ElementType,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        length: usize,
    ) -> Result<Val<'js, Self>, Error>;

    fn element_type<'js>(
        token: Tok<'js, Self>,
        typed_array: &Val<'js, Self>,
    ) -> Result<ElementType, Error>;
    fn buffer<'js>(token: Tok<'js, Self>, view: &Val<'js, Self>) -> Result<Val<'js, Self>, Error>;
    fn byte_offset<'js>(token: Tok<'js, Self>, view: &Val<'js, Self>) -> Result<usize, Error>;
    fn byte_length<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<usize, Error>;

    fn read<'js>(
        token: Tok<'js, Self>,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        byte_length: usize,
    ) -> Result<Vec<u8>, Error>;
    fn write<'js>(
        token: Tok<'js, Self>,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        bytes: &[u8],
    ) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use crate::backend::buffers::ElementType;

    #[test]
    fn element_types_have_their_javascript_sizes() {
        assert_eq!(ElementType::Uint8Clamped.size(), 1);
        assert_eq!(ElementType::Float16.size(), 2);
        assert_eq!(ElementType::Float32.size(), 4);
        assert_eq!(ElementType::BigUint64.size(), 8);
    }

    #[test]
    fn element_types_name_their_constructors() {
        assert_eq!(ElementType::Uint8Clamped.name(), "Uint8ClampedArray");
        assert_eq!(ElementType::BigInt64.name(), "BigInt64Array");
    }

    #[test]
    fn all_lists_each_element_type_once() {
        assert!(
            ElementType::ALL
                .iter()
                .enumerate()
                .all(|(index, element)| !ElementType::ALL[..index].contains(element))
        );
    }
}
