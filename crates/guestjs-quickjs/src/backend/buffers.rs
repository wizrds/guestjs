use std::{
    ffi::c_int,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};

use guestjs_core::{
    backend::{
        Tok, Val,
        buffers::{BackendBuffers, ElementType},
        values::{BackendValues, Kind},
    },
    errors::{BorrowKind, Error},
};
use rquickjs::{ArrayBuffer, Exception, Function, Value, function::This, qjs};

use crate::{
    backend::QuickJs,
    context::Token,
    engine::{LoanGuard, Storage},
    error::Capture,
};

pub struct PropertyAtom<'js> {
    token: Token<'js>,
    atom: qjs::JSAtom,
}

impl<'js> PropertyAtom<'js> {
    pub fn new(token: Token<'js>, name: &str) -> rquickjs::Result<Self> {
        match unsafe {
            qjs::JS_NewAtomLen(token.pointer().as_ptr(), name.as_ptr().cast(), name.len() as _)
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
            qjs::JS_FreeAtom(self.token.pointer().as_ptr(), self.atom);
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

    fn buffer_range(&self, token: Token<'js>) -> Result<(Value<'js>, usize, usize), Error>;
}

impl<'js> ArrayBufferData<'js> for Value<'js> {
    fn array_buffer(&self, token: Token<'js>) -> rquickjs::Result<(*mut u8, usize)> {
        let mut size = 0;

        match unsafe { qjs::JS_IsArrayBuffer(self.as_raw()) } {
            false => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer")),
            true => match unsafe {
                qjs::JS_GetArrayBuffer(token.pointer().as_ptr(), &mut size, self.as_raw())
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

    fn buffer_range(&self, token: Token<'js>) -> Result<(Value<'js>, usize, usize), Error> {
        match QuickJs::kind(token, self) {
            Kind::ArrayBuffer => Ok((self.clone(), 0, QuickJs::byte_length(token, self)?)),
            Kind::TypedArray | Kind::DataView => match QuickJs::buffer(token, self)? {
                buffer if QuickJs::kind(token, &buffer) == Kind::ArrayBuffer => Ok((
                    buffer,
                    QuickJs::byte_offset(token, self)?,
                    QuickJs::byte_length(token, self)?,
                )),
                _ => {
                    Err(Error::conversion("view of an ArrayBuffer", "view of a SharedArrayBuffer"))
                }
            },
            _ => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer or ArrayBuffer view"))
                .capture(token),
        }
    }
}

pub trait TypedArrayBuffer<'js> {
    fn typed_array_buffer(&self, token: Token<'js>)
    -> rquickjs::Result<(Value<'js>, usize, usize)>;
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
    fn intrinsic(&self, token: Token<'js>, name: &str) -> rquickjs::Result<Value<'js>>;
}

impl<'js> IntrinsicAccessor<'js> for Value<'js> {
    fn intrinsic(&self, token: Token<'js>, name: &str) -> rquickjs::Result<Value<'js>> {
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
            0 => Err(Exception::throw_type(&token.ctx(), "intrinsic accessor is missing")),
            _ => Err(rquickjs::Error::Exception),
        }
    }
}

pub struct Bytes<'a> {
    _guard: Option<LoanGuard>,
    bytes: NonNull<[u8]>,
    source: PhantomData<&'a [u8]>,
}

impl<'a> Bytes<'a> {
    pub fn new<'js>(
        token: Token<'js>,
        source: &'a Value<'js>,
        kind: BorrowKind,
    ) -> Result<Self, Error> {
        let (buffer, byte_offset, byte_length) = source.buffer_range(token)?;
        let (data, size) = buffer
            .array_buffer(token)
            .capture(token)?;

        if byte_offset
            .checked_add(byte_length)
            .is_none_or(|end| end > size)
        {
            return Err(Exception::throw_range(
                &token.ctx(),
                "byte range is outside the ArrayBuffer",
            ))
            .capture(token);
        }

        if kind == BorrowKind::Exclusive
            && unsafe { qjs::JS_IsImmutableArrayBuffer(buffer.as_raw()) } == 1
        {
            return Err(Exception::throw_type(&token.ctx(), "ArrayBuffer is immutable"))
                .capture(token);
        }

        match byte_length {
            0 => Ok(Self {
                _guard: None,
                bytes: NonNull::from(&mut [][..]),
                source: PhantomData,
            }),
            _ => token
                .loans()
                .lend(Storage::new("ArrayBuffer", data.addr()), kind)
                .map(|guard| Self {
                    _guard: Some(guard),
                    bytes: NonNull::slice_from_raw_parts(
                        unsafe { NonNull::new_unchecked(data.add(byte_offset)) },
                        byte_length,
                    ),
                    source: PhantomData,
                }),
        }
    }

    pub unsafe fn as_mut(&mut self) -> &mut [u8] {
        unsafe { self.bytes.as_mut() }
    }
}

impl Deref for Bytes<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { self.bytes.as_ref() }
    }
}

pub struct BytesMut<'a>(Bytes<'a>);

impl Deref for BytesMut<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl DerefMut for BytesMut<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { self.0.as_mut() }
    }
}

impl BackendBuffers for QuickJs {
    type Bytes<'a> = Bytes<'a>;
    type BytesMut<'a> = BytesMut<'a>;

    fn new_array_buffer<'js>(token: Tok<'js, Self>, bytes: &[u8]) -> Result<Val<'js, Self>, Error> {
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
                element.raw() as c_int == unsafe { qjs::JS_GetTypedArrayType(typed_array.as_raw()) }
            })
            .ok_or_else(|| Exception::throw_type(&token.ctx(), "not a TypedArray"))
            .capture(token)
    }

    fn buffer<'js>(token: Tok<'js, Self>, view: &Val<'js, Self>) -> Result<Val<'js, Self>, Error> {
        match Self::kind(token, view) {
            Kind::TypedArray => view
                .typed_array_buffer(token)
                .map(|(buffer, _, _)| buffer),
            Kind::DataView => view.intrinsic(token, "buffer"),
            _ => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer view")),
        }
        .capture(token)
    }

    fn byte_offset<'js>(token: Tok<'js, Self>, view: &Val<'js, Self>) -> Result<usize, Error> {
        match Self::kind(token, view) {
            Kind::TypedArray => view
                .typed_array_buffer(token)
                .map(|(_, byte_offset, _)| byte_offset),
            Kind::DataView => view
                .intrinsic(token, "byteOffset")
                .and_then(|byte_offset| byte_offset.get::<u32>())
                .map(|byte_offset| byte_offset as usize),
            _ => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer view")),
        }
        .capture(token)
    }

    fn byte_length<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<usize, Error> {
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
            _ => Err(Exception::throw_type(&token.ctx(), "not an ArrayBuffer or ArrayBuffer view")),
        }
        .capture(token)
    }

    fn borrow_bytes<'js, 'a>(
        token: Tok<'js, Self>,
        source: &'a Val<'js, Self>,
    ) -> Result<Self::Bytes<'a>, Error> {
        Bytes::new(token, source, BorrowKind::Shared)
    }

    fn borrow_bytes_mut<'js, 'a>(
        token: Tok<'js, Self>,
        source: &'a Val<'js, Self>,
    ) -> Result<Self::BytesMut<'a>, Error> {
        Bytes::new(token, source, BorrowKind::Exclusive).map(BytesMut)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::{
        backend::{
            Backend,
            buffers::{BackendBuffers, ElementType},
            values::BackendValues,
        },
        errors::Error,
    };
    use rquickjs::{Value, qjs};

    use crate::{backend::QuickJs, engine::Config};

    guestjs_core::fixtures::buffers::tests!(QuickJs);

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
            assert_eq!(&QuickJs::borrow_bytes(token, &buffer).unwrap()[2..], [3, 4, 5, 6, 7, 8]);
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
    fn borrow_bytes_mut_copies_into_the_buffer() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[0; 4]).unwrap();

            QuickJs::borrow_bytes_mut(token, &buffer).unwrap()[1..3].copy_from_slice(&[7, 8]);

            assert_eq!(&QuickJs::borrow_bytes(token, &buffer).unwrap()[..], [0, 7, 8, 0]);
        });
    }

    #[test]
    fn byte_access_rejects_invalid_buffers() {
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
            let detached =
                eval("const detached = new ArrayBuffer(4); detached.transfer(); detached");

            assert!(QuickJs::borrow_bytes(token, &detached).is_err_and(|error| error.is_guest()));
            assert!(
                QuickJs::byte_length(token, &detached)
                    .unwrap_err()
                    .is_guest()
            );
            assert!(
                QuickJs::borrow_bytes(token, &eval("new SharedArrayBuffer(4)"))
                    .is_err_and(|error| error.is_guest())
            );
            assert!(
                QuickJs::byte_length(token, &eval("({ byteLength: 4 })"))
                    .unwrap_err()
                    .is_guest()
            );

            unsafe { qjs::JS_SetImmutableArrayBuffer(buffer.as_raw(), true) };

            assert!(QuickJs::borrow_bytes_mut(token, &buffer).is_err_and(|error| error.is_guest()));
            assert!(!token.ctx().has_exception());
        });
    }

    #[test]
    fn borrow_bytes_views_a_typed_array_window() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[1, 2, 3, 4, 5, 6]).unwrap();

            assert_eq!(
                &QuickJs::borrow_bytes(
                    token,
                    &QuickJs::new_typed_array(token, ElementType::Uint8, &buffer, 2, 3).unwrap(),
                )
                .unwrap()[..],
                [3, 4, 5],
            );
        });
    }

    #[test]
    fn borrow_bytes_mut_writes_through_to_the_buffer() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::borrow_bytes_mut(
                token,
                &token
                    .ctx()
                    .eval::<Value, _>("globalThis.bytes = new Uint8Array(4); bytes")
                    .unwrap(),
            )
            .unwrap()[2] = 9;

            assert_eq!(
                token
                    .ctx()
                    .eval::<i32, _>("bytes[2]")
                    .unwrap(),
                9,
            );
        });
    }

    #[test]
    fn borrow_bytes_mut_refuses_an_immutable_buffer() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[0; 4]).unwrap();

            unsafe { qjs::JS_SetImmutableArrayBuffer(buffer.as_raw(), true) };

            assert!(matches!(
                QuickJs::borrow_bytes_mut(token, &buffer),
                Err(Error::Guest(exception)) if exception.name() == "TypeError",
            ));
        });
    }

    #[test]
    fn borrow_bytes_refuses_a_shared_array_buffer_view() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(matches!(
                QuickJs::borrow_bytes(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>("new Uint8Array(new SharedArrayBuffer(4))")
                        .unwrap(),
                ),
                Err(Error::Conversion { found, .. }) if found == "view of a SharedArrayBuffer",
            ));
        });
    }

    #[test]
    fn a_second_mutable_loan_is_refused() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[0; 4]).unwrap();
            let first = QuickJs::new_typed_array(token, ElementType::Uint8, &buffer, 0, 2).unwrap();
            let _guard = QuickJs::borrow_bytes_mut(token, &first).unwrap();

            assert!(matches!(
                QuickJs::borrow_bytes_mut(
                    token,
                    &QuickJs::new_typed_array(token, ElementType::Uint8, &buffer, 2, 2).unwrap(),
                ),
                Err(Error::Borrow { .. }),
            ));
        });
    }

    #[test]
    fn guest_code_is_refused_while_lent() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[1]).unwrap();
            let function = token
                .ctx()
                .eval::<Value, _>("globalThis.calls = 0; () => { calls += 1; }")
                .unwrap();
            let guard = QuickJs::borrow_bytes(token, &buffer).unwrap();

            assert!(matches!(
                QuickJs::call(token, &function, &QuickJs::new_undefined(token), &[]),
                Err(Error::Borrow { .. }),
            ));

            drop(guard);

            assert_eq!(
                token
                    .ctx()
                    .eval::<i32, _>("calls")
                    .unwrap(),
                0,
            );
        });
    }

    #[test]
    fn guest_code_runs_after_the_loan_drops() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[1]).unwrap();
            let function = token
                .ctx()
                .eval::<Value, _>("globalThis.calls = 0; () => { calls += 1; }")
                .unwrap();

            drop(QuickJs::borrow_bytes(token, &buffer).unwrap());

            QuickJs::call(token, &function, &QuickJs::new_undefined(token), &[]).unwrap();

            assert_eq!(
                token
                    .ctx()
                    .eval::<i32, _>("calls")
                    .unwrap(),
                1,
            );
        });
    }

    #[test]
    fn empty_buffers_are_not_lent() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let buffer = QuickJs::new_array_buffer(token, &[]).unwrap();
            let _guard = QuickJs::borrow_bytes_mut(token, &buffer).unwrap();

            assert!(token.loans().settled().is_ok());
        });
    }
}
