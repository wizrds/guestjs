use guestjs_core::{
    backend::{
        values::{
            BackendValues,
            Kind,
            Property,
            WellKnown,
        },
        Tok,
        Val,
    },
    errors::Error,
};
use rquickjs::{
    atom::PredefinedAtom,
    function::Args,
    object::{
        AsProperty,
        PropertyFlags,
    },
    qjs,
    Array,
    Atom,
    BigInt,
    Coerced,
    Constructor,
    Ctx,
    Filter,
    Function,
    IntoJs,
    IteratorJs,
    Object,
    Symbol,
    Type,
    Value,
};

use crate::{
    backend::QuickJs,
    error::Capture,
};

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
                    | flag(configurable, qjs::JS_PROP_CONFIGURABLE)) as PropertyFlags,
                value,
                undefined.clone(),
                undefined,
            ),
            Property::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => (
                (qjs::JS_PROP_HAS_GET
                    | qjs::JS_PROP_HAS_SET
                    | qjs::JS_PROP_HAS_ENUMERABLE
                    | qjs::JS_PROP_HAS_CONFIGURABLE
                    | flag(enumerable, qjs::JS_PROP_ENUMERABLE)
                    | flag(configurable, qjs::JS_PROP_CONFIGURABLE)) as PropertyFlags,
                undefined.clone(),
                get.unwrap_or_else(|| undefined.clone()),
                set.unwrap_or(undefined),
            ),
        })
    }
}

impl BackendValues for QuickJs {
    fn undefined<'js>(token: Tok<'js, Self>) -> Val<'js, Self> {
        Value::new_undefined(token.ctx())
    }

    fn null<'js>(token: Tok<'js, Self>) -> Val<'js, Self> {
        Value::new_null(token.ctx())
    }

    fn boolean<'js>(token: Tok<'js, Self>, value: bool) -> Val<'js, Self> {
        Value::new_bool(token.ctx(), value)
    }

    fn number<'js>(token: Tok<'js, Self>, value: f64) -> Val<'js, Self> {
        Value::new_number(token.ctx(), value)
    }

    fn bigint<'js>(
        token: Tok<'js, Self>,
        value: i64,
    ) -> Result<Val<'js, Self>, Error> {
        BigInt::from_i64(token.ctx(), value)
            .map(BigInt::into_value)
            .capture(token)
    }

    fn string<'js>(
        token: Tok<'js, Self>,
        value: &str,
    ) -> Result<Val<'js, Self>, Error> {
        value
            .into_js(&token.ctx())
            .capture(token)
    }

    fn symbol<'js>(
        token: Tok<'js, Self>,
        description: &str,
    ) -> Result<Val<'js, Self>, Error> {
        Symbol::with_description(token.ctx(), description)
            .map(Symbol::into_value)
            .capture(token)
    }

    fn well_known<'js>(
        token: Tok<'js, Self>,
        symbol: WellKnown,
    ) -> Val<'js, Self> {
        Atom::from_predefined(
            token.ctx(),
            match symbol {
                WellKnown::Iterator => PredefinedAtom::SymbolIterator,
                WellKnown::AsyncIterator => PredefinedAtom::SymbolAsyncIterator,
                WellKnown::ToPrimitive => PredefinedAtom::SymbolToPrimitive,
                WellKnown::ToStringTag => PredefinedAtom::SymbolToStringTag,
                WellKnown::HasInstance => PredefinedAtom::SymbolHasInstance,
            },
        )
        .to_value()
        .expect("well-known symbols are predefined")
    }

    fn array<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error> {
        items
            .into_iter()
            .collect_js::<Array>(&token.ctx())
            .map(Array::into_value)
            .capture(token)
    }

    fn object<'js>(token: Tok<'js, Self>) -> Result<Val<'js, Self>, Error> {
        Object::new(token.ctx())
            .map(Object::into_value)
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
            _ if unsafe { qjs::JS_IsArrayBuffer(value.as_raw()) } => {
                Kind::ArrayBuffer
            }
            _ if unsafe { qjs::JS_GetTypedArrayType(value.as_raw()) } >= 0 => {
                Kind::TypedArray
            }
            _ if unsafe { qjs::JS_IsDataView(value.as_raw()) } => Kind::DataView,
            _ => Kind::Object,
        }
    }

    fn as_bool<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<bool, Error> {
        value
            .get::<bool>()
            .capture(token)
    }

    fn as_f64<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<f64, Error> {
        value
            .get::<f64>()
            .capture(token)
    }

    fn as_i64<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<i64, Error> {
        match value.type_of() {
            Type::BigInt => BigInt::from_value(value.clone())
                .and_then(BigInt::to_i64)
                .capture(token)
                .and_then(|integer| {
                    match Self::same(token, value, &Self::bigint(token, integer)?) {
                        true => Ok(integer),
                        false => Err(Error::conversion(
                            "i64",
                            "bigint out of range",
                        )),
                    }
                }),
            _ => value
                .as_number()
                .filter(|number| {
                    number.fract() == 0.0
                        && (i64::MIN as f64..i64::MAX as f64).contains(number)
                })
                .map(|number| number as i64)
                .ok_or_else(|| Error::conversion("i64", value.type_name())),
        }
    }

    fn as_string<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<String, Error> {
        value
            .get::<String>()
            .capture(token)
    }

    fn display<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<String, Error> {
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

    fn same<'js>(
        token: Tok<'js, Self>,
        first: &Val<'js, Self>,
        second: &Val<'js, Self>,
    ) -> bool {
        unsafe {
            qjs::JS_IsSameValue(
                token
                    .pointer()
                    .as_ptr(),
                first.as_raw(),
                second.as_raw(),
            )
        }
    }

    fn get<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
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
        Object::from_value(object.clone())
            .and_then(|object| object.prop(key.clone(), Descriptor(property)))
            .capture(token)
    }

    fn delete<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<(), Error> {
        Object::from_value(object.clone())
            .and_then(|object| object.remove(key.clone()))
            .capture(token)
    }

    fn has<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<bool, Error> {
        Object::from_value(object.clone())
            .and_then(|object| object.contains_key(key.clone()))
            .capture(token)
    }

    fn keys<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Vec<Val<'js, Self>>, Error> {
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
        match unsafe {
            qjs::JS_IsInstanceOf(
                token
                    .pointer()
                    .as_ptr(),
                value.as_raw(),
                constructor.as_raw(),
            )
        } {
            -1 => Err(rquickjs::Error::Exception).capture(token),
            result => Ok(result == 1),
        }
    }

    fn prototype_of<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error> {
        Object::from_value(object.clone())
            .map(|object| {
                object
                    .get_prototype()
                    .map_or_else(
                        || Self::null(token),
                        Object::into_value,
                    )
            })
            .capture(token)
    }

    fn set_prototype<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        prototype: &Val<'js, Self>,
    ) -> Result<(), Error> {
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
        Constructor::from_value(constructor.clone())
            .and_then(|constructor| {
                let mut arguments = Args::new(token.ctx(), args.len());

                arguments.push_args(args.iter().cloned())?;

                constructor.construct_args(arguments)
            })
            .capture(token)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::backend::{
        values::{
            BackendValues,
            Kind,
            Property,
        },
        Backend,
    };
    use rquickjs::Value;

    use crate::{
        backend::QuickJs,
        engine::Config,
    };


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
            assert_eq!(kind("new Proxy(new ArrayBuffer(1), {})"), Kind::Object);
            assert_eq!(kind("new Error()"), Kind::Object);
            assert!(!token
                .ctx()
                .has_exception());
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
            assert!(
                as_i64("2n ** 64n")
                    .is_err_and(|error| error.is_conversion()),
            );
            assert!(as_i64("'42'").is_err_and(|error| error.is_conversion()));
        });
    }

    #[test]
    fn delete_throws_on_non_configurable() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let object = QuickJs::object(token).unwrap();
            let key = QuickJs::string(token, "fixed").unwrap();

            QuickJs::define(
                token,
                &object,
                &key,
                Property::Value {
                    value: QuickJs::number(token, 1.0),
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

            assert_eq!(QuickJs::keys(token, &object).unwrap().len(), 2);
        });
    }

    #[test]
    fn same_follows_same_value() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(QuickJs::same(
                token,
                &QuickJs::number(token, f64::NAN),
                &QuickJs::number(token, f64::NAN),
            ));
            assert!(!QuickJs::same(
                token,
                &QuickJs::number(token, 0.0),
                &QuickJs::number(token, -0.0),
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
                .eval::<Value, _>(
                    "({ [Symbol.hasInstance]: (value) => value % 2 === 0 })",
                )
                .unwrap();

            assert!(QuickJs::instance_of(
                token,
                &QuickJs::number(token, 2.0),
                &even,
            )
            .unwrap());
            assert!(!QuickJs::instance_of(
                token,
                &QuickJs::number(token, 3.0),
                &even,
            )
            .unwrap());
        });
    }
}
