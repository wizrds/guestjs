use std::vec::IntoIter;

use serde::{
    Deserialize,
    de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, Unexpected, Visitor},
    forward_to_deserialize_any,
    ser::{
        self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
        SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    },
};

use crate::{
    backend::{Backend, Val, buffers::BackendBuffers, values::Kind},
    enter::Enter,
    errors::Error,
    marshal::{
        FromGuest, FromGuestRef, ToGuest,
        collections::{Elements, Entries},
        describe::{Describe, Expected},
        primitives::MAX_SAFE_INTEGER,
    },
};

pub struct Serializer<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
}

impl<'a, 'js, B: BackendBuffers> Serializer<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>) -> Self {
        Self { enter }
    }

    pub fn variant(&self, variant: &str, value: Val<'js, B>) -> Result<Val<'js, B>, Error> {
        let object = B::new_object(self.enter.token())?;

        B::set(self.enter.token(), &object, &variant.to_guest(self.enter)?, value)?;

        Ok(object)
    }
}

impl<'a, 'js, B: BackendBuffers> ser::Serializer for Serializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;
    type SerializeSeq = ArraySerializer<'a, 'js, B>;
    type SerializeTuple = ArraySerializer<'a, 'js, B>;
    type SerializeTupleStruct = ArraySerializer<'a, 'js, B>;
    type SerializeTupleVariant = VariantSerializer<'a, 'js, B, ArraySerializer<'a, 'js, B>>;
    type SerializeMap = ObjectSerializer<'a, 'js, B>;
    type SerializeStruct = ObjectSerializer<'a, 'js, B>;
    type SerializeStructVariant = VariantSerializer<'a, 'js, B, ObjectSerializer<'a, 'js, B>>;

    fn serialize_bool(self, value: bool) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_i8(self, value: i8) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_i16(self, value: i16) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_i32(self, value: i32) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_i64(self, value: i64) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_u8(self, value: u8) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_u16(self, value: u16) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_u32(self, value: u32) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_u64(self, value: u64) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_f32(self, value: f32) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_f64(self, value: f64) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_char(self, value: char) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_str(self, value: &str) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Val<'js, B>, Error> {
        value.to_guest(self.enter)
    }

    fn serialize_none(self) -> Result<Val<'js, B>, Error> {
        None::<()>.to_guest(self.enter)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Val<'js, B>, Error>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Val<'js, B>, Error> {
        ().to_guest(self.enter)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Val<'js, B>, Error> {
        ().to_guest(self.enter)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Val<'js, B>, Error> {
        variant.to_guest(self.enter)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Val<'js, B>, Error>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Val<'js, B>, Error>
    where
        T: Serialize + ?Sized,
    {
        self.variant(variant, value.serialize(Serializer::new(self.enter))?)
    }

    fn serialize_seq(self, _length: Option<usize>) -> Result<ArraySerializer<'a, 'js, B>, Error> {
        Ok(ArraySerializer::new(self.enter))
    }

    fn serialize_tuple(self, _length: usize) -> Result<ArraySerializer<'a, 'js, B>, Error> {
        Ok(ArraySerializer::new(self.enter))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<ArraySerializer<'a, 'js, B>, Error> {
        Ok(ArraySerializer::new(self.enter))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Ok(VariantSerializer::new(self.enter, variant, ArraySerializer::new(self.enter)))
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<ObjectSerializer<'a, 'js, B>, Error> {
        ObjectSerializer::new(self.enter)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<ObjectSerializer<'a, 'js, B>, Error> {
        ObjectSerializer::new(self.enter)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Ok(VariantSerializer::new(self.enter, variant, ObjectSerializer::new(self.enter)?))
    }
}

pub struct ArraySerializer<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    items: Vec<Val<'js, B>>,
}

impl<'a, 'js, B: BackendBuffers> ArraySerializer<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>) -> Self {
        Self { enter, items: Vec::new() }
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeSeq for ArraySerializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        value
            .serialize(Serializer::new(self.enter))
            .map(|item| self.items.push(item))
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        B::new_array(self.enter.token(), self.items)
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeTuple for ArraySerializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        SerializeSeq::end(self)
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeTupleStruct for ArraySerializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        SerializeSeq::end(self)
    }
}

pub struct ObjectSerializer<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    object: Val<'js, B>,
    key: Option<Val<'js, B>>,
}

impl<'a, 'js, B: BackendBuffers> ObjectSerializer<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>) -> Result<Self, Error> {
        Ok(Self {
            enter,
            object: B::new_object(enter.token())?,
            key: None,
        })
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeMap for ObjectSerializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        key.serialize(Serializer::new(self.enter))
            .map(|key| self.key = Some(key))
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        B::set(
            self.enter.token(),
            &self.object,
            &self
                .key
                .take()
                .ok_or_else(|| Error::unexpected("a map value was serialized before its key"))?,
            value.serialize(Serializer::new(self.enter))?,
        )
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        Ok(self.object)
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeStruct for ObjectSerializer<'a, 'js, B> {
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        SerializeMap::end(self)
    }
}

pub struct VariantSerializer<'a, 'js, B: Backend, S> {
    enter: &'a Enter<'js, B>,
    variant: &'static str,
    inner: S,
}

impl<'a, 'js, B: BackendBuffers, S> VariantSerializer<'a, 'js, B, S> {
    pub fn new(enter: &'a Enter<'js, B>, variant: &'static str, inner: S) -> Self {
        Self { enter, variant, inner }
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeTupleVariant
    for VariantSerializer<'a, 'js, B, ArraySerializer<'a, 'js, B>>
{
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        SerializeSeq::serialize_element(&mut self.inner, value)
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        Serializer::new(self.enter).variant(self.variant, SerializeSeq::end(self.inner)?)
    }
}

impl<'a, 'js, B: BackendBuffers> SerializeStructVariant
    for VariantSerializer<'a, 'js, B, ObjectSerializer<'a, 'js, B>>
{
    type Ok = Val<'js, B>;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
    {
        SerializeStruct::serialize_field(&mut self.inner, key, value)
    }

    fn end(self) -> Result<Val<'js, B>, Error> {
        Serializer::new(self.enter).variant(self.variant, SerializeStruct::end(self.inner)?)
    }
}

pub struct Deserializer<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    value: Val<'js, B>,
}

impl<'a, 'js, B: BackendBuffers> Deserializer<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>, value: Val<'js, B>) -> Self {
        Self { enter, value }
    }
}

impl<B: Backend> Describe for Deserializer<'_, '_, B> {
    fn describe(expected: &mut Expected) {
        expected.push("undefined");
        expected.push("null");
        expected.push("boolean");
        expected.push("number");
        expected.push("bigint");
        expected.push("string");
        expected.push("array");
        expected.push("ArrayBuffer");
        expected.push("typed array");
        expected.push("DataView");
        expected.push("object");
    }
}

impl<'de, 'a, 'js, B: BackendBuffers> de::Deserializer<'de> for Deserializer<'a, 'js, B> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match B::kind(self.enter.token(), &self.value) {
            Kind::Undefined | Kind::Null => visitor.visit_unit(),
            Kind::Boolean => visitor.visit_bool(bool::from_guest(self.enter, self.value)?),
            Kind::Number => {
                let number = f64::from_guest(self.enter, self.value)?;

                match number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER {
                    true => visitor.visit_i64(number as i64),
                    false => visitor.visit_f64(number),
                }
            }
            Kind::BigInt => visitor.visit_i64(i64::from_guest(self.enter, self.value)?),
            Kind::String => visitor
                .visit_str(&<str as FromGuestRef<B>>::from_guest_ref(self.enter, &self.value)?),
            Kind::Array => visitor
                .visit_seq(ArrayAccess::new(self.enter, Elements::read(self.enter, &self.value)?)),
            Kind::ArrayBuffer | Kind::TypedArray | Kind::DataView => visitor
                .visit_bytes(&<[u8] as FromGuestRef<B>>::from_guest_ref(self.enter, &self.value)?),
            Kind::Object => visitor
                .visit_map(ObjectAccess::new(self.enter, Entries::read(self.enter, &self.value)?)),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match B::kind(self.enter.token(), &self.value) {
            Kind::Undefined | Kind::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match B::kind(self.enter.token(), &self.value) {
            Kind::String => {
                visitor.visit_enum(String::from_guest(self.enter, self.value)?.into_deserializer())
            }
            Kind::Object => visitor.visit_enum(VariantAccess::new(
                self.enter,
                Entries::read(self.enter, &self.value)?.exact::<1>()?,
            )),
            _ => de::Deserializer::deserialize_any(self, visitor),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

pub struct ArrayAccess<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    items: IntoIter<Val<'js, B>>,
}

impl<'a, 'js, B: BackendBuffers> ArrayAccess<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>, elements: Elements<'js, B>) -> Self {
        Self { enter, items: elements.into_iter() }
    }
}

impl<'de, 'a, 'js, B: BackendBuffers> de::SeqAccess<'de> for ArrayAccess<'a, 'js, B> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.items
            .next()
            .map(|item| seed.deserialize(Deserializer::new(self.enter, item)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

pub struct ObjectAccess<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    entries: IntoIter<(Val<'js, B>, Val<'js, B>)>,
    value: Option<Val<'js, B>>,
}

impl<'a, 'js, B: BackendBuffers> ObjectAccess<'a, 'js, B> {
    pub fn new(enter: &'a Enter<'js, B>, entries: Entries<'js, B>) -> Self {
        Self {
            enter,
            entries: entries.into_iter(),
            value: None,
        }
    }
}

impl<'de, 'a, 'js, B: BackendBuffers> de::MapAccess<'de> for ObjectAccess<'a, 'js, B> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Error>
    where
        K: DeserializeSeed<'de>,
    {
        self.entries
            .next()
            .map(|(key, value)| {
                self.value = Some(value);

                seed.deserialize(Deserializer::new(self.enter, key))
            })
            .transpose()
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Error>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(Deserializer::new(
            self.enter,
            self.value
                .take()
                .ok_or_else(|| Error::unexpected("a map value was deserialized before its key"))?,
        ))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

pub struct VariantAccess<'a, 'js, B: Backend> {
    enter: &'a Enter<'js, B>,
    variant: Val<'js, B>,
    value: Val<'js, B>,
}

impl<'a, 'js, B: BackendBuffers> VariantAccess<'a, 'js, B> {
    pub fn new(
        enter: &'a Enter<'js, B>,
        [(variant, value)]: [(Val<'js, B>, Val<'js, B>); 1],
    ) -> Self {
        Self { enter, variant, value }
    }
}

impl<'de, 'a, 'js, B: BackendBuffers> de::EnumAccess<'de> for VariantAccess<'a, 'js, B> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self), Error>
    where
        V: DeserializeSeed<'de>,
    {
        Ok((seed.deserialize(Deserializer::new(self.enter, self.variant.clone()))?, self))
    }
}

impl<'de, 'a, 'js, B: BackendBuffers> de::VariantAccess<'de> for VariantAccess<'a, 'js, B> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        <()>::deserialize(Deserializer::new(self.enter, self.value))
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(Deserializer::new(self.enter, self.value))
    }

    fn tuple_variant<V>(self, _length: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(Deserializer::new(self.enter, self.value), visitor)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_map(Deserializer::new(self.enter, self.value), visitor)
    }
}

pub struct Serde<T>(T);

impl<T> Serde<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
}

impl<B, T> ToGuest<B> for Serde<T>
where
    B: BackendBuffers,
    T: Serialize,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.0.serialize(Serializer::new(enter))
    }
}

impl<B, T> FromGuest<B> for Serde<T>
where
    B: BackendBuffers,
    T: DeserializeOwned + 'static,
{
    type Owned = T;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        T::deserialize(Deserializer::new(enter, value))
    }
}

impl ser::Error for Error {
    fn custom<T>(message: T) -> Self
    where
        T: std::fmt::Display,
    {
        Self::unexpected(message.to_string())
    }
}

impl de::Error for Error {
    fn custom<T>(message: T) -> Self
    where
        T: std::fmt::Display,
    {
        Self::unexpected(message.to_string())
    }

    fn invalid_type(unexpected: Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        Self::conversion(expected.to_string(), unexpected.to_string())
    }

    fn invalid_value(unexpected: Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        <Self as de::Error>::invalid_type(unexpected, expected)
    }
}

#[cfg(test)]
mod tests {
    use serde::de::{self, Unexpected};

    use crate::{
        backend::{Backend, buffers::BackendBuffers},
        errors::Error,
        marshal::{FromGuest, ToGuest, serde::Serde},
    };

    #[allow(dead_code)]
    fn conversions<B: BackendBuffers>() {
        fn to<B: Backend, T: ToGuest<B>>() {}
        fn from<B: Backend, T: FromGuest<B, Owned = O>, O>() {}

        to::<B, Serde<Vec<i64>>>();
        from::<B, Serde<Vec<i64>>, Vec<i64>>();
    }

    #[test]
    fn invalid_type_is_a_conversion() {
        assert_eq!(
            <Error as de::Error>::invalid_type(Unexpected::Str("one"), &"a number",).to_string(),
            "expected a number, found string \"one\"",
        );
    }

    #[test]
    fn custom_errors_are_unexpected() {
        assert!(<Error as de::Error>::custom("missing field `name`").is_unexpected(),);
    }
}
