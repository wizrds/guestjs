use std::{
    ffi::c_int,
    mem::MaybeUninit,
    ptr,
    slice,
};

use guestjs_core::{
    backend::{
        buffers::{
            BackendBuffers,
            ElementType,
        },
        values::{
            BackendValues,
            Kind,
        },
        Tok,
        Val,
    },
    errors::Error,
};
use rquickjs::{
    function::This,
    qjs,
    ArrayBuffer,
    Exception,
    Function,
    Value,
};

use crate::{
    backend::QuickJs,
    context::Token,
    error::Capture,
};

pub struct PropertyAtom<'js> {
    token: Token<'js>,
    atom: qjs::JSAtom,
}

impl<'js> PropertyAtom<'js> {
    pub fn new(
        token: Token<'js>,
        name: &str,
    ) -> rquickjs::Result<Self> {
        match unsafe {
            qjs::JS_NewAtomLen(
                token.pointer().as_ptr(),
                name.as_ptr().cast(),
                name.len() as _,
            )
        } {
            qjs::JS_ATOM_NULL => Err(rquickjs::Error::Exception),
            atom => Ok(Self { token, atom }),
        }
    }

    pub fn as_raw(&self) -> qjs::JSAtom {
        self.atom
    }
}

impl Drop for PropertyAtom<'_> {
    fn drop(&mut self) {
        unsafe {
            qjs::JS_FreeAtom(
                self.token.pointer().as_ptr(),
                self.atom,
            );
        }
    }
}

pub trait RawElementType {
    fn raw(self) -> qjs::JSTypedArrayEnum;
}

impl RawElementType for ElementType {
    fn raw(self) -> qjs::JSTypedArrayEnum {
        match self {
            ElementType::Int8 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_INT8,
            ElementType::Uint8 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_UINT8,
            ElementType::Uint8Clamped => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_UINT8C,
            ElementType::Int16 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_INT16,
            ElementType::Uint16 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_UINT16,
            ElementType::Int32 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_INT32,
            ElementType::Uint32 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_UINT32,
            ElementType::BigInt64 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_BIG_INT64,
            ElementType::BigUint64 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_BIG_UINT64,
            ElementType::Float16 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_FLOAT16,
            ElementType::Float32 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_FLOAT32,
            ElementType::Float64 => qjs::JSTypedArrayEnum_JS_TYPED_ARRAY_FLOAT64,
        }
    }
}

pub trait ArrayBufferData<'js> {
    fn array_buffer(&self, token: Token<'js>) -> rquickjs::Result<(*mut u8, usize)>;

    unsafe fn range(
        &self,
        token: Token<'js>,
        byte_offset: usize,
        length: usize,
    ) -> rquickjs::Result<&mut [u8]>;
}

impl<'js> ArrayBufferData<'js> for Value<'js> {
    fn array_buffer(&self, token: Token<'js>) -> rquickjs::Result<(*mut u8, usize)> {
        let mut size = 0;

        match unsafe { qjs::JS_IsArrayBuffer(self.as_raw()) } {
            false => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer")),
            true => match unsafe {
                qjs::JS_GetArrayBuffer(
                    token.pointer().as_ptr(),
                    &mut size,
                    self.as_raw(),
                )
            } {
                data if data.is_null()
                    && unsafe { qjs::JS_HasException(token.pointer().as_ptr()) } =>
                {
                    Err(rquickjs::Error::Exception)
                }
                data => Ok((data, size as usize)),
            },
        }
    }

    unsafe fn range(
        &self,
        token: Token<'js>,
        byte_offset: usize,
        length: usize,
    ) -> rquickjs::Result<&mut [u8]> {
        match self.array_buffer(token)? {
            (_, size) if byte_offset
                .checked_add(length)
                .is_none_or(|end| end > size) =>
            {
                Err(Exception::throw_range(
                    &token.ctx(),
                    "byte range is outside the ArrayBuffer",
                ))
            }
            _ if length == 0 => Ok(&mut []),
            (data, _) => Ok(unsafe {
                slice::from_raw_parts_mut(data.add(byte_offset), length)
            }),
        }
    }
}

pub trait TypedArrayBuffer<'js> {
    fn typed_array_buffer(
        &self,
        token: Token<'js>,
    ) -> rquickjs::Result<(Value<'js>, usize, usize)>;
}

impl<'js> TypedArrayBuffer<'js> for Value<'js> {
    fn typed_array_buffer(
        &self,
        token: Token<'js>,
    ) -> rquickjs::Result<(Value<'js>, usize, usize)> {
        let mut byte_offset = 0;
        let mut byte_length = 0;

        match unsafe {
            qjs::JS_GetTypedArrayBuffer(
                token.pointer().as_ptr(),
                self.as_raw(),
                &mut byte_offset,
                &mut byte_length,
                ptr::null_mut(),
            )
        } {
            raw if unsafe { qjs::JS_IsException(raw) } => Err(rquickjs::Error::Exception),
            raw => Ok((
                unsafe { Value::from_raw(token.ctx(), raw) },
                byte_offset as usize,
                byte_length as usize,
            )),
        }
    }
}

pub trait IntrinsicAccessor<'js> {
    fn intrinsic(
        &self,
        token: Token<'js>,
        name: &str,
    ) -> rquickjs::Result<Value<'js>>;
}

impl<'js> IntrinsicAccessor<'js> for Value<'js> {
    fn intrinsic(
        &self,
        token: Token<'js>,
        name: &str,
    ) -> rquickjs::Result<Value<'js>> {
        let mut descriptor = MaybeUninit::<qjs::JSPropertyDescriptor>::uninit();

        match unsafe {
            qjs::JS_GetOwnProperty(
                token.pointer().as_ptr(),
                descriptor.as_mut_ptr(),
                Value::from_raw(
                    token.ctx(),
                    qjs::JS_GetClassProto(
                        token.pointer().as_ptr(),
                        qjs::JS_GetClassID(self.as_raw()),
                    ),
                )
                .as_raw(),
                PropertyAtom::new(token, name)?.as_raw(),
            )
        } {
            1 => unsafe {
                let descriptor = descriptor.assume_init();

                drop(Value::from_raw(token.ctx(), descriptor.value));
                drop(Value::from_raw(token.ctx(), descriptor.setter));

                Function::from_value(Value::from_raw(token.ctx(), descriptor.getter))
            }
            .and_then(|getter| getter.call::<_, Value>((This(self.clone()),))),
            0 => Err(Exception::throw_type(
                &token.ctx(),
                "intrinsic accessor is missing",
            )),
            _ => Err(rquickjs::Error::Exception),
        }
    }
}

impl BackendBuffers for QuickJs {
    fn new_array_buffer<'js>(
        token: Tok<'js, Self>,
        bytes: &[u8],
    ) -> Result<Val<'js, Self>, Error> {
        ArrayBuffer::new_copy(token.ctx(), bytes)
            .map(ArrayBuffer::into_value)
            .capture(token)
    }

    fn new_typed_array<'js>(
        token: Tok<'js, Self>,
        element: ElementType,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        length: usize,
    ) -> Result<Val<'js, Self>, Error> {
        match unsafe {
            qjs::JS_NewTypedArray(
                token.pointer().as_ptr(),
                3,
                [
                    buffer.as_raw(),
                    Self::new_number(token, byte_offset as f64).as_raw(),
                    Self::new_number(token, length as f64).as_raw(),
                ]
                .as_mut_ptr(),
                element.raw(),
            )
        } {
            raw if unsafe { qjs::JS_IsException(raw) } => Err(rquickjs::Error::Exception),
            raw => Ok(unsafe { Value::from_raw(token.ctx(), raw) }),
        }
        .capture(token)
    }

    fn element_type<'js>(
        token: Tok<'js, Self>,
        typed_array: &Val<'js, Self>,
    ) -> Result<ElementType, Error> {
        ElementType::ALL
            .into_iter()
            .find(|element| {
                element.raw() as c_int
                    == unsafe { qjs::JS_GetTypedArrayType(typed_array.as_raw()) }
            })
            .ok_or_else(|| Exception::throw_type(&token.ctx(), "not a TypedArray"))
            .capture(token)
    }

    fn buffer<'js>(
        token: Tok<'js, Self>,
        view: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        match Self::kind(token, view) {
            Kind::TypedArray => view
                .typed_array_buffer(token)
                .map(|(buffer, _, _)| buffer),
            Kind::DataView => view.intrinsic(token, "buffer"),
            _ => Err(Exception::throw_type(
                &token.ctx(),
                "not an ArrayBuffer view",
            )),
        }
        .capture(token)
    }

    fn byte_offset<'js>(
        token: Tok<'js, Self>,
        view: &Val<'js, Self>,
    ) -> Result<usize, Error> {
        match Self::kind(token, view) {
            Kind::TypedArray => view
                .typed_array_buffer(token)
                .map(|(_, byte_offset, _)| byte_offset),
            Kind::DataView => view
                .intrinsic(token, "byteOffset")
                .and_then(|byte_offset| byte_offset.get::<u32>())
                .map(|byte_offset| byte_offset as usize),
            _ => Err(Exception::throw_type(
                &token.ctx(),
                "not an ArrayBuffer view",
            )),
        }
        .capture(token)
    }

    fn byte_length<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<usize, Error> {
        match Self::kind(token, value) {
            Kind::ArrayBuffer => value
                .array_buffer(token)
                .map(|(_, size)| size),
            Kind::TypedArray => value
                .typed_array_buffer(token)
                .map(|(_, _, byte_length)| byte_length),
            Kind::DataView => value
                .intrinsic(token, "byteLength")
                .and_then(|byte_length| byte_length.get::<u32>())
                .map(|byte_length| byte_length as usize),
            _ => Err(Exception::throw_type(
                &token.ctx(),
                "not an ArrayBuffer or ArrayBuffer view",
            )),
        }
        .capture(token)
    }

    fn read<'js>(
        token: Tok<'js, Self>,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        byte_length: usize,
    ) -> Result<Vec<u8>, Error> {
        unsafe { buffer.range(token, byte_offset, byte_length) }
            .map(|range| range.to_vec())
            .capture(token)
    }

    fn write<'js>(
        token: Tok<'js, Self>,
        buffer: &Val<'js, Self>,
        byte_offset: usize,
        bytes: &[u8],
    ) -> Result<(), Error> {
        match unsafe { qjs::JS_IsImmutableArrayBuffer(buffer.as_raw()) } {
            1 => Err(Exception::throw_type(
                &token.ctx(),
                "ArrayBuffer is immutable",
            )),
            _ => unsafe { buffer.range(token, byte_offset, bytes.len()) }
                .map(|range| range.copy_from_slice(bytes)),
        }
        .capture(token)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::backend::{
        buffers::{
            BackendBuffers,
            ElementType,
        },
        values::BackendValues,
        Backend,
    };
    use rquickjs::{
        qjs,
        Value,
    };

    use crate::{
        backend::QuickJs,
        engine::Config,
    };

    #[test]
    fn typed_arrays_view_a_new_array_buffer() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
            let view = QuickJs::new_typed_array(token, ElementType::Uint16, &buffer, 2, 3).unwrap();

            assert_eq!(QuickJs::element_type(token, &view).unwrap(), ElementType::Uint16);
            assert!(QuickJs::same(token, &QuickJs::buffer(token, &view).unwrap(), &buffer));
            assert_eq!(QuickJs::byte_offset(token, &view).unwrap(), 2);
            assert_eq!(QuickJs::byte_length(token, &view).unwrap(), 6);
            assert_eq!(QuickJs::byte_length(token, &buffer).unwrap(), 8);
            assert_eq!(QuickJs::read(token, &buffer, 2, 6).unwrap(), vec![3, 4, 5, 6, 7, 8]);
        });
    }

    #[test]
    fn element_type_maps_every_element_type() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[0; 8]).unwrap();

            for element in ElementType::ALL {
                assert_eq!(
                    QuickJs::element_type(
                        token,
                        &QuickJs::new_typed_array(token, element, &buffer, 0, 1).unwrap(),
                    )
                    .unwrap(),
                    element,
                );
            }
        });
    }

    #[test]
    fn data_views_report_their_slots_not_their_own_properties() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let view = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    const view = new DataView(new ArrayBuffer(8), 2, 4);
                    Object.defineProperty(view, "byteLength", { value: 99 });
                    globalThis.DataView = undefined;
                    view;
                    "#,
                )
                .unwrap();

            assert_eq!(QuickJs::byte_offset(token, &view).unwrap(), 2);
            assert_eq!(QuickJs::byte_length(token, &view).unwrap(), 4);
            assert_eq!(
                QuickJs::byte_length(token, &QuickJs::buffer(token, &view).unwrap()).unwrap(),
                8,
            );
        });
    }

    #[test]
    fn write_copies_into_the_buffer() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[0; 4]).unwrap();

            QuickJs::write(token, &buffer, 1, &[7, 8]).unwrap();

            assert_eq!(QuickJs::read(token, &buffer, 0, 4).unwrap(), vec![0, 7, 8, 0]);
        });
    }

    #[test]
    fn byte_access_rejects_invalid_buffers_and_ranges() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let eval = |source: &str| {
                token
                    .ctx()
                    .eval::<Value, _>(source)
                    .unwrap()
            };
            let buffer = QuickJs::new_array_buffer(token, &[0; 4]).unwrap();
            let detached = eval("const detached = new ArrayBuffer(4); detached.transfer(); detached");

            assert!(QuickJs::read(token, &buffer, 2, 4)
                .unwrap_err()
                .is_guest());
            assert!(QuickJs::read(token, &detached, 0, 0)
                .unwrap_err()
                .is_guest());
            assert!(QuickJs::byte_length(token, &detached)
                .unwrap_err()
                .is_guest());
            assert!(QuickJs::read(token, &eval("new SharedArrayBuffer(4)"), 0, 0)
                .unwrap_err()
                .is_guest());
            assert!(QuickJs::byte_length(token, &eval("({ byteLength: 4 })"))
                .unwrap_err()
                .is_guest());

            unsafe { qjs::JS_SetImmutableArrayBuffer(buffer.as_raw(), true) };

            assert!(QuickJs::write(token, &buffer, 0, &[1])
                .unwrap_err()
                .is_guest());
            assert!(!token
                .ctx()
                .has_exception());
        });
    }
}
