use crate::{
    backend::{
        Backend,
        Tok,
        Val,
    },
    errors::Error,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Undefined,
    Null,
    Boolean,
    Number,
    BigInt,
    String,
    Symbol,
    Array,
    Function,
    Promise,
    ArrayBuffer,
    TypedArray,
    DataView,
    Object,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WellKnown {
    Iterator,
    AsyncIterator,
    ToPrimitive,
    ToStringTag,
    HasInstance,
}

pub enum Property<'js, B: Backend> {
    Value {
        value: Val<'js, B>,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    },
    Accessor {
        get: Option<Val<'js, B>>,
        set: Option<Val<'js, B>>,
        enumerable: bool,
        configurable: bool,
    },
}

pub trait BackendValues: Backend {
    fn undefined<'js>(token: Tok<'js, Self>) -> Val<'js, Self>;
    fn null<'js>(token: Tok<'js, Self>) -> Val<'js, Self>;
    fn boolean<'js>(token: Tok<'js, Self>, value: bool) -> Val<'js, Self>;
    fn number<'js>(token: Tok<'js, Self>, value: f64) -> Val<'js, Self>;
    fn bigint<'js>(
        token: Tok<'js, Self>,
        value: i64,
    ) -> Result<Val<'js, Self>, Error>;
    fn string<'js>(
        token: Tok<'js, Self>,
        value: &str,
    ) -> Result<Val<'js, Self>, Error>;
    fn symbol<'js>(
        token: Tok<'js, Self>,
        description: &str,
    ) -> Result<Val<'js, Self>, Error>;
    fn well_known<'js>(
        token: Tok<'js, Self>,
        symbol: WellKnown,
    ) -> Val<'js, Self>;
    fn array<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error>;
    fn object<'js>(token: Tok<'js, Self>) -> Result<Val<'js, Self>, Error>;

    fn kind<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Kind;
    fn as_bool<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<bool, Error>;
    fn as_f64<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<f64, Error>;
    fn as_i64<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<i64, Error>;
    fn as_string<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<String, Error>;
    fn display<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<String, Error>;
    fn same<'js>(
        token: Tok<'js, Self>,
        first: &Val<'js, Self>,
        second: &Val<'js, Self>,
    ) -> bool;

    fn get<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error>;
    fn set<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
        value: Val<'js, Self>,
    ) -> Result<(), Error>;
    fn define<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
        property: Property<'js, Self>,
    ) -> Result<(), Error>;
    fn delete<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<(), Error>;
    fn has<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        key: &Val<'js, Self>,
    ) -> Result<bool, Error>;
    fn keys<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Vec<Val<'js, Self>>, Error>;
    fn instance_of<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
        constructor: &Val<'js, Self>,
    ) -> Result<bool, Error>;

    fn call<'js>(
        token: Tok<'js, Self>,
        function: &Val<'js, Self>,
        this: &Val<'js, Self>,
        args: &[Val<'js, Self>],
    ) -> Result<Val<'js, Self>, Error>;
    fn construct<'js>(
        token: Tok<'js, Self>,
        constructor: &Val<'js, Self>,
        args: &[Val<'js, Self>],
    ) -> Result<Val<'js, Self>, Error>;
}
