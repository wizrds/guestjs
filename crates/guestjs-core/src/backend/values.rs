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
    Set,
    Object,
}

impl Kind {
    pub fn is_object(self) -> bool {
        !matches!(
            self,
            Kind::Undefined
                | Kind::Null
                | Kind::Boolean
                | Kind::Number
                | Kind::BigInt
                | Kind::String
                | Kind::Symbol
        )
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WellKnown {
    Iterator,
    AsyncIterator,
    ToPrimitive,
    ToStringTag,
    HasInstance,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IteratorKind {
    Sync,
    Async,
}

impl IteratorKind {
    pub fn symbol(self) -> WellKnown {
        match self {
            IteratorKind::Sync => WellKnown::Iterator,
            IteratorKind::Async => WellKnown::AsyncIterator,
        }
    }
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
    fn new_undefined<'js>(token: Tok<'js, Self>) -> Val<'js, Self>;
    fn new_null<'js>(token: Tok<'js, Self>) -> Val<'js, Self>;
    fn new_boolean<'js>(token: Tok<'js, Self>, value: bool) -> Val<'js, Self>;
    fn new_number<'js>(token: Tok<'js, Self>, value: f64) -> Val<'js, Self>;
    fn new_bigint<'js>(
        token: Tok<'js, Self>,
        value: i64,
    ) -> Result<Val<'js, Self>, Error>;
    fn new_string<'js>(
        token: Tok<'js, Self>,
        value: &str,
    ) -> Result<Val<'js, Self>, Error>;
    fn new_symbol<'js>(
        token: Tok<'js, Self>,
        description: &str,
    ) -> Result<Val<'js, Self>, Error>;
    fn new_array<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error>;
    fn new_object<'js>(token: Tok<'js, Self>) -> Result<Val<'js, Self>, Error>;
    fn new_set<'js>(
        token: Tok<'js, Self>,
        items: Vec<Val<'js, Self>>,
    ) -> Result<Val<'js, Self>, Error>;
    fn well_known<'js>(
        token: Tok<'js, Self>,
        symbol: WellKnown,
    ) -> Val<'js, Self>;

    fn kind<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Kind;
    fn truthy<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Result<bool, Error>;
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
    fn prototype_of<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error>;
    fn set_prototype<'js>(
        token: Tok<'js, Self>,
        object: &Val<'js, Self>,
        prototype: &Val<'js, Self>,
    ) -> Result<(), Error>;

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

    fn iter<'js>(
        token: Tok<'js, Self>,
        iterable: &Val<'js, Self>,
        kind: IteratorKind,
    ) -> Result<Option<Val<'js, Self>>, Error>;
    fn next<'js>(
        token: Tok<'js, Self>,
        iterator: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error>;
    fn step<'js>(
        token: Tok<'js, Self>,
        result: &Val<'js, Self>,
    ) -> Result<Option<Val<'js, Self>>, Error>;
    fn close<'js>(
        token: Tok<'js, Self>,
        iterator: &Val<'js, Self>,
    ) -> Result<Val<'js, Self>, Error>;
}

#[cfg(test)]
mod tests {
    use crate::backend::values::{
        IteratorKind,
        WellKnown,
    };

    #[test]
    fn iterator_kinds_name_their_well_known_symbol() {
        assert_eq!(IteratorKind::Sync.symbol(), WellKnown::Iterator);
        assert_eq!(IteratorKind::Async.symbol(), WellKnown::AsyncIterator);
    }
}
