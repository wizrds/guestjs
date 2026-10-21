use std::{ops::Deref, ptr::NonNull, slice};

use guestjs_core::{
    backend::{
        Tok, Val,
        values::{BackendValues, IteratorKind, Kind, Property, WellKnown},
    },
    errors::Error,
};
use rquickjs::{
    Array, BigInt, Coerced, Constructor, Ctx, Exception, Filter, Function, IntoJs, IteratorJs,
    Object, Symbol, Type, Value,
    atom::PredefinedAtom,
    function::{Args, This},
    object::{AsProperty, PropertyFlags},
    qjs::{self, c_char},
};

use crate::{backend::QuickJs, context::Token, error::Capture};

pub struct Descriptor<'js>(Property<'js, QuickJs>);

impl<'js> AsProperty<'js, ()> for Descriptor<'js> {
    fn config(
        self,
        ctx: &Ctx<'js>,
    ) -> rquickjs::Result<(PropertyFlags, Value<'js>, Value<'js>, Value<'js>)> {
        let flag = |set: bool, flag: u32| match set {
            true => flag,
            false => 0,
        };

        let undefined = Value::new_undefined(ctx.clone());

        Ok(match self.0 {
            Property::Value {
                value,
                writable,
                enumerable,
                configurable,
            } => (
                (qjs::JS_PROP_HAS_VALUE
                    | qjs::JS_PROP_HAS_WRITABLE
                    | qjs::JS_PROP_HAS_ENUMERABLE
                    | qjs::JS_PROP_HAS_CONFIGURABLE
                    | flag(writable, qjs::JS_PROP_WRITABLE)
                    | flag(enumerable, qjs::JS_PROP_ENUMERABLE)
                    | flag(configurable, qjs::JS_PROP_CONFIGURABLE))
                    as PropertyFlags,
                value,
                undefined.clone(),
                undefined,
            ),
            Property::Accessor { get, set, enumerable, configurable } => (
                (qjs::JS_PROP_HAS_GET
                    | qjs::JS_PROP_HAS_SET
                    | qjs::JS_PROP_HAS_ENUMERABLE
                    | qjs::JS_PROP_HAS_CONFIGURABLE
                    | flag(enumerable, qjs::JS_PROP_ENUMERABLE)
                    | flag(configurable, qjs::JS_PROP_CONFIGURABLE))
                    as PropertyFlags,
                undefined.clone(),
                get.unwrap_or_else(|| undefined.clone()),
                set.unwrap_or(undefined),
            ),
        })
    }
}

pub struct Str<'a> {
    context: NonNull<qjs::JSContext>,
    pointer: *const c_char,
    text: &'a str,
}

impl<'a> Str<'a> {
    pub fn new<'js>(token: Token<'js>, value: &'a Value<'js>) -> Result<Self, Error> {
        let mut length = 0;

        match unsafe {
            qjs::JS_ToCStringLen2(token.pointer().as_ptr(), &mut length, value.as_raw(), false)
        } {
            pointer if pointer.is_null() => Err(rquickjs::Error::Exception).capture(token),
            pointer => match str::from_utf8(unsafe {
                slice::from_raw_parts(pointer.cast(), length as usize)
            }) {
                Ok(text) => Ok(Self { context: token.pointer(), pointer, text }),
                Err(_) => {
                    unsafe { qjs::JS_FreeCString(token.pointer().as_ptr(), pointer) };

                    Err(Error::conversion("UTF-8 string", "string with lone surrogates"))
                }
            },
        }
    }
}

impl Deref for Str<'_> {
    type Target = str;

    fn deref(&self) -> &str {
        self.text
    }
}

impl Drop for Str<'_> {
    fn drop(&mut self) {
        unsafe { qjs::JS_FreeCString(self.context.as_ptr(), self.pointer) };
    }
}

impl BackendValues for QuickJs {
    type Str<'a> = Str<'a>;

    fn new_undefined<'js>(token: Tok<'js, Self>) -> Val<'js, Self> {
        Value::new_undefined(token.ctx())
    }

    fn new_null<'js>(token: Tok<'js, Self>) -> Val<'js, Self> {
        Value::new_null(token.ctx())
    }

    fn new_boolean<'js>(token: Tok<'js, Self>, value: bool) -> Val<'js, Self> {
        Value::new_bool(token.ctx(), value)
    }

    fn new_number<'js>(token: Tok<'js, Self>, value: f64) -> Val<'js, Self> {
        Value::new_float(token.ctx(), value)
    }

    fn new_bigint<'js>(token: Tok<'js, Self>, value: i64) -> Result<Val<'js, Self>, Error> {
        BigInt::from_i64(token.ctx(), value)
            .map(BigInt::into_value)
            .capture(token)
    }

    fn new_string<'js>(token: Tok<'js, Self>, value: &str) -> Result<Val<'js, Self>, Error> {
        value
            .into_js(&token.ctx())
            .capture(token)
    }

    fn new_symbol<'js>(token: Tok<'js, Self>, description: &str) -> Result<Val<'js, Self>, Error> {
        Symbol::with_description(token.ctx(), description)
            .map(Symbol::into_value)
            .capture(token)
    }

    fn well_known<'js>(token: Tok<'js, Self>, symbol: WellKnown) -> Val<'js, Self> {
        match symbol {
            WellKnown::Iterator => Symbol::iterator(token.ctx()).into_value(),
            WellKnown::AsyncIterator => Symbol::async_iterator(token.ctx()).into_value(),
            WellKnown::ToPrimitive => Symbol::to_primitive(token.ctx()).into_value(),
            WellKnown::HasInstance => Symbol::has_instance(token.ctx()).into_value(),
            WellKnown::ToStringTag => unsafe {
                Value::from_raw(
                    token.ctx(),
                    qjs::JS_AtomToValue(
                        token.ctx().as_raw().as_ptr(),
                        PredefinedAtom::SymbolToStringTag as qjs::JSAtom,
                    ),
                )
            },
        }
    }

    fn new_array<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error> {
        items
            .into_iter()
            .collect_js::<Array>(&token.ctx())
            .map(Array::into_value)
            .capture(token)
    }

    fn new_object<'js>(token: Tok<'js, Self>) -> Result<Val<'js, Self>, Error> {
        Object::new(token.ctx())
            .map(Object::into_value)
            .capture(token)
    }

    fn new_set<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error> {
        token
            .ctx()
            .globals()
            .get::<_, Constructor>(PredefinedAtom::Set)
            .and_then(|set| {
                set.construct::<_, Value>((items
                    .into_iter()
                    .collect_js::<Array>(&token.ctx())?,))
            })
            .capture(token)
    }

    fn kind<'js>(_token: Tok<'js, Self>, value: &Val<'js, Self>) -> Kind {
        match value.type_of() {
            Type::Uninitialized | Type::Undefined => Kind::Undefined,
            Type::Null => Kind::Null,
            Type::Bool => Kind::Boolean,
            Type::Int | Type::Float => Kind::Number,
            Type::BigInt => Kind::BigInt,
            Type::String => Kind::String,
            Type::Symbol => Kind::Symbol,
            Type::Array => Kind::Array,
            Type::Constructor | Type::Function => Kind::Function,
            Type::Promise => Kind::Promise,
            _ if unsafe { qjs::JS_IsArrayBuffer(value.as_raw()) } => Kind::ArrayBuffer,
            _ if unsafe { qjs::JS_GetTypedArrayType(value.as_raw()) } >= 0 => Kind::TypedArray,
            _ if unsafe { qjs::JS_IsDataView(value.as_raw()) } => Kind::DataView,
            _ if unsafe { qjs::JS_IsSet(value.as_raw()) } => Kind::Set,
            _ => Kind::Object,
        }
    }

    fn truthy<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<bool, Error> {
        value
            .get::<Coerced<bool>>()
            .map(|truthy| truthy.0)
            .capture(token)
    }

    fn as_bool<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<bool, Error> {
        value.get::<bool>().capture(token)
    }

    fn as_f64<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<f64, Error> {
        value.get::<f64>().capture(token)
    }

    fn as_i64<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<i64, Error> {
        match value.type_of() {
            Type::BigInt => BigInt::from_value(value.clone())
                .and_then(BigInt::to_i64)
                .capture(token)
                .and_then(|integer| {
                    match Self::same(token, value, &Self::new_bigint(token, integer)?) {
                        true => Ok(integer),
                        false => Err(Error::conversion("i64", "bigint out of range")),
                    }
                }),
            _ => value
                .as_number()
                .filter(|number| {
                    number.fract() == 0.0 && (i64::MIN as f64..i64::MAX as f64).contains(number)
                })
                .map(|number| number as i64)
                .ok_or_else(|| Error::conversion("i64", value.type_name())),
        }
    }

    fn borrow_str<'js, 'a>(
        token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::Str<'a>, Error> {
        match Self::kind(token, value) {
            Kind::String => Str::new(token, value),
            _ => Err(Exception::throw_type(&token.ctx(), "not a string")).capture(token),
        }
    }

    fn display<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Result<String, Error> {
        token.loans().settled()?;

        match Symbol::from_value(value.clone()) {
            Ok(symbol) => symbol
                .description()
                .and_then(|description| description.get::<Option<String>>())
                .map(|description| format!("Symbol({})", description.unwrap_or_default())),
            Err(_) => value
                .get::<Coerced<String>>()
                .map(|string| string.0),
        }
        .capture(token)
    }

    fn same<'js>(token: Tok<'js, Self>, first: &Val<'js, Self>, second: &Val<'js, Self>) -> bool {
        unsafe { qjs::JS_IsSameValue(token.pointer().as_ptr(), first.as_raw(), second.as_raw()) }
    }

    fn get<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.get(key.clone()))
            .capture(token)
    }

    fn set<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
        value: Val<'js, Self>,
    ) -> Result<(), Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.set(key.clone(), value))
            .capture(token)
    }

    fn define<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
        property: Property<'js, Self>,
    ) -> Result<(), Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.prop(key.clone(), Descriptor(property)))
            .capture(token)
    }

    fn delete<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<(), Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.remove(key.clone()))
            .capture(token)
    }

    fn has<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<bool, Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.contains_key(key.clone()))
            .capture(token)
    }

    fn keys<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Vec<Val<'js, Self>>, Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| {
                object
                    .own_keys::<Value>(
                        Filter::new()
                            .string()
                            .symbol()
                            .enum_only(),
                    )
                    .collect()
            })
            .capture(token)
    }

    fn instance_of<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
        constructor: &Val<'js, Self>,
    ) -> Result<bool, Error> {
        token.loans().settled()?;

        match unsafe {
            qjs::JS_IsInstanceOf(token.pointer().as_ptr(), value.as_raw(), constructor.as_raw())
        } {
            -1 => Err(rquickjs::Error::Exception).capture(token),
            result => Ok(result == 1),
        }
    }

    fn prototype_of<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .map(|object| {
                object
                    .get_prototype()
                    .map_or_else(|| Self::new_null(token), Object::into_value)
            })
            .capture(token)
    }

    fn set_prototype<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        prototype: &Val<'js, Self>,
    ) -> Result<(), Error> {
        token.loans().settled()?;

        Object::from_value(object.clone())
            .and_then(|object| object.set_prototype(prototype.as_object()))
            .capture(token)
    }

    fn call<'js>(
        token: Tok<'js, Self>,
        function: &Val<'js, Self>,
        this: &Val<'js, Self>,
        args: &[Val<'js, Self>],
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Function::from_value(function.clone())
            .and_then(|function| {
                let mut arguments = Args::new(token.ctx(), args.len());

                arguments.this(this.clone())?;
                arguments.push_args(args.iter().cloned())?;

                function.call_arg(arguments)
            })
            .capture(token)
    }

    fn construct<'js>(
        token: Tok<'js, Self>,
        constructor: &Val<'js, Self>,
        args: &[Val<'js, Self>],
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Constructor::from_value(constructor.clone())
            .and_then(|constructor| {
                let mut arguments = Args::new(token.ctx(), args.len());

                arguments.push_args(args.iter().cloned())?;

                constructor.construct_args(arguments)
            })
            .capture(token)
    }

    fn iter<'js>(
        token: Tok<'js, Self>,
        iterable: &Val<'js, Self>,
        kind: IteratorKind,
    ) -> Result<Option<Val<'js, Self>>, Error> {
        token.loans().settled()?;

        iterable
            .as_object()
            .map(|object| {
                object
                    .get::<_, Option<Function>>(Self::well_known(token, kind.symbol()))?
                    .map(|method| method.call::<_, Value>((This(object.clone()),)))
                    .transpose()
            })
            .transpose()
            .map(Option::flatten)
            .capture(token)
    }

    fn next<'js>(
        token: Tok<'js, Self>,
        iterator: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Object::from_value(iterator.clone())
            .and_then(|iterator| {
                iterator
                    .get::<_, Function>(PredefinedAtom::Next)?
                    .call::<_, Value>((This(iterator),))
            })
            .capture(token)
    }

    fn step<'js>(
        token: Tok<'js, Self>,
        result: &Val<'js, Self>,
    ) -> Result<Option<Val<'js, Self>>, Error> {
        token.loans().settled()?;

        Object::from_value(result.clone())
            .and_then(|result| {
                match result
                    .get::<_, Coerced<bool>>(PredefinedAtom::Done)?
                    .0
                {
                    true => Ok(None),
                    false => result
                        .get(PredefinedAtom::Value)
                        .map(Some),
                }
            })
            .capture(token)
    }

    fn close<'js>(
        token: Tok<'js, Self>,
        iterator: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        Object::from_value(iterator.clone())
            .and_then(|iterator| {
                match iterator.get::<_, Option<Function>>(PredefinedAtom::Return)? {
                    Some(method) => method.call::<_, Value>((This(iterator),)),
                    None => Ok(Value::new_undefined(token.ctx())),
                }
            })
            .capture(token)
    }
}

#[cfg(test)]
mod tests {
    use std::iter::from_fn;

    use guestjs_core::backend::{
        Backend,
        values::{BackendValues, IteratorKind, Kind, Property},
    };
    use rquickjs::Value;

    use crate::{backend::QuickJs, engine::Config};

    guestjs_core::fixtures::values::tests!(QuickJs);

    #[test]
    fn kind_classifies_values() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let kind = |source: &str| {
                QuickJs::kind(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(source)
                        .unwrap(),
                )
            };

            assert_eq!(kind("undefined"), Kind::Undefined);
            assert_eq!(kind("1.5"), Kind::Number);
            assert_eq!(kind("1n"), Kind::BigInt);
            assert_eq!(kind("[]"), Kind::Array);
            assert_eq!(kind("(class {})"), Kind::Function);
            assert_eq!(kind("Promise.resolve()"), Kind::Promise);
            assert_eq!(kind("new ArrayBuffer(1)"), Kind::ArrayBuffer);
            assert_eq!(kind("new SharedArrayBuffer(1)"), Kind::Object);
            assert_eq!(kind("new Float16Array(1)"), Kind::TypedArray);
            assert_eq!(kind("new DataView(new ArrayBuffer(1))"), Kind::DataView);
            assert_eq!(kind("new Set()"), Kind::Set);
            assert_eq!(kind("new Proxy(new ArrayBuffer(1), {})"), Kind::Object);
            assert_eq!(kind("new Error()"), Kind::Object);
            assert!(!token.ctx().has_exception());
        });
    }

    #[test]
    fn as_i64_is_strict() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let as_i64 = |source: &str| {
                QuickJs::as_i64(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(source)
                        .unwrap(),
                )
            };

            assert_eq!(as_i64("42").unwrap(), 42);
            assert_eq!(as_i64("42n").unwrap(), 42);
            assert!(as_i64("1.5").is_err_and(|error| error.is_conversion()));
            assert!(as_i64("2n ** 64n").is_err_and(|error| error.is_conversion()),);
            assert!(as_i64("'42'").is_err_and(|error| error.is_conversion()));
        });
    }

    #[test]
    fn delete_throws_on_non_configurable() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let object = QuickJs::new_object(token).unwrap();
            let key = QuickJs::new_string(token, "fixed").unwrap();

            QuickJs::define(
                token,
                &object,
                &key,
                Property::Value {
                    value: QuickJs::new_number(token, 1.0),
                    writable: false,
                    enumerable: true,
                    configurable: false,
                },
            )
            .unwrap();

            assert!(QuickJs::delete(token, &object, &key).is_err());
            assert!(QuickJs::has(token, &object, &key).unwrap());
        });
    }

    #[test]
    fn truthy_follows_to_boolean() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let truthy = |source: &str| {
                QuickJs::truthy(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(source)
                        .unwrap(),
                )
                .unwrap()
            };

            assert!(!truthy("0"));
            assert!(!truthy("''"));
            assert!(!truthy("null"));
            assert!(truthy("'0'"));
            assert!(truthy("({})"));
            assert!(truthy("[]"));
        });
    }

    #[test]
    fn iter_opens_only_values_with_the_protocol_method() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let opens = |source: &str, kind: IteratorKind| {
                QuickJs::iter(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(source)
                        .unwrap(),
                    kind,
                )
                .unwrap()
                .is_some()
            };

            assert!(opens("[1]", IteratorKind::Sync));
            assert!(opens("new Set([1])", IteratorKind::Sync));
            assert!(opens("(async function* () {})()", IteratorKind::Async));
            assert!(!opens("[1]", IteratorKind::Async));
            assert!(!opens("({})", IteratorKind::Sync));
            assert!(!opens("1", IteratorKind::Sync));
        });
    }

    #[test]
    fn next_and_step_walk_a_set_in_insertion_order() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let iterator = QuickJs::iter(
                token,
                &QuickJs::new_set(
                    token,
                    vec![
                        QuickJs::new_number(token, 2.0),
                        QuickJs::new_number(token, 1.0),
                        QuickJs::new_number(token, 2.0),
                    ],
                )
                .unwrap(),
                IteratorKind::Sync,
            )
            .unwrap()
            .unwrap();

            assert_eq!(
                from_fn(|| {
                    QuickJs::step(token, &QuickJs::next(token, &iterator).unwrap()).unwrap()
                })
                .map(|value| QuickJs::as_f64(token, &value).unwrap())
                .collect::<Vec<_>>(),
                vec![2.0, 1.0],
            );
        });
    }

    #[test]
    fn step_reads_done_with_to_boolean() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let yields = |source: &str| {
                QuickJs::step(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(source)
                        .unwrap(),
                )
                .unwrap()
                .is_some()
            };

            assert!(!yields("({ done: true, value: 5 })"));
            assert!(!yields("({ done: 1, value: 5 })"));
            assert!(yields("({ done: false, value: 5 })"));
            assert!(yields("({ done: 0, value: 5 })"));
            assert!(yields("({ value: 5 })"));
        });
    }

    #[test]
    fn close_runs_return_when_the_iterator_has_one() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let generator = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    globalThis.closed = false;
                    (function* () {
                        try {
                            yield 1;
                        } finally {
                            globalThis.closed = true;
                        }
                    })()
                    "#,
                )
                .unwrap();

            QuickJs::next(token, &generator).unwrap();
            QuickJs::close(token, &generator).unwrap();

            assert!(
                token
                    .ctx()
                    .eval::<bool, _>("globalThis.closed")
                    .unwrap(),
            );
            assert_eq!(
                QuickJs::kind(
                    token,
                    &QuickJs::close(
                        token,
                        &token
                            .ctx()
                            .eval::<Value, _>("[][Symbol.iterator]()")
                            .unwrap(),
                    )
                    .unwrap(),
                ),
                Kind::Undefined,
            );
        });
    }

    #[test]
    fn keys_include_symbols_and_skip_hidden() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let object = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    const object = { a: 1, [Symbol.iterator]: 2 };
                    Object.defineProperty(object, "hidden", { value: 3 });
                    object
                    "#,
                )
                .unwrap();

            assert_eq!(
                QuickJs::keys(token, &object)
                    .unwrap()
                    .len(),
                2
            );
        });
    }

    #[test]
    fn same_follows_same_value() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(QuickJs::same(
                token,
                &QuickJs::new_number(token, f64::NAN),
                &QuickJs::new_number(token, f64::NAN),
            ));
            assert!(!QuickJs::same(
                token,
                &QuickJs::new_number(token, 0.0),
                &QuickJs::new_number(token, -0.0),
            ));
        });
    }

    #[test]
    fn instance_of_runs_has_instance_for_primitives() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let even = token
                .ctx()
                .eval::<Value, _>("({ [Symbol.hasInstance]: (value) => value % 2 === 0 })")
                .unwrap();

            assert!(QuickJs::instance_of(token, &QuickJs::new_number(token, 2.0), &even,).unwrap());
            assert!(
                !QuickJs::instance_of(token, &QuickJs::new_number(token, 3.0), &even,).unwrap()
            );
        });
    }

    #[test]
    fn borrow_str_views_a_string() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            for text in ["plain ascii", "naïve ☃ 𝄞"] {
                assert_eq!(
                    &*QuickJs::borrow_str(token, &QuickJs::new_string(token, text).unwrap())
                        .unwrap(),
                    text,
                );
            }
        });
    }

    #[test]
    fn borrow_str_refuses_an_object_without_calling_to_string() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(
                QuickJs::borrow_str(
                    token,
                    &token
                        .ctx()
                        .eval::<Value, _>(
                            r#"
                            globalThis.calls = 0;
                            ({ toString() { calls += 1; return "called"; } })
                            "#,
                        )
                        .unwrap(),
                )
                .is_err()
            );
            assert_eq!(
                token
                    .ctx()
                    .eval::<i32, _>("calls")
                    .unwrap(),
                0,
            );
        });
    }
}
