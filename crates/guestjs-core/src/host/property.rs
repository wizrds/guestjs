use std::rc::Rc;

use crate::{
    backend::{
        Backend, Val,
        callables::BackendCallables,
        values::{BackendValues, Property, WellKnown},
    },
    enter::Enter,
    errors::Error,
    host::{declaration::Declaration, function::FunctionDeclaration, state::HostState},
    marshal::ToGuest,
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PropertyKey {
    Name(String),
    Symbol(WellKnown),
}

impl PropertyKey {
    pub fn function_name(&self) -> String {
        match self {
            Self::Name(name) => name.clone(),
            Self::Symbol(symbol) => format!(
                "[Symbol.{}]",
                match symbol {
                    WellKnown::Iterator => "iterator",
                    WellKnown::AsyncIterator => "asyncIterator",
                    WellKnown::ToPrimitive => "toPrimitive",
                    WellKnown::ToStringTag => "toStringTag",
                    WellKnown::HasInstance => "hasInstance",
                },
            ),
        }
    }
}

impl From<&str> for PropertyKey {
    fn from(name: &str) -> Self {
        Self::Name(name.to_owned())
    }
}

impl From<String> for PropertyKey {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}

impl From<WellKnown> for PropertyKey {
    fn from(symbol: WellKnown) -> Self {
        Self::Symbol(symbol)
    }
}

impl<B: BackendValues> ToGuest<B> for &PropertyKey {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        match self {
            PropertyKey::Name(name) => name.as_str().to_guest(enter),
            PropertyKey::Symbol(symbol) => Ok(B::well_known(enter.token(), *symbol)),
        }
    }
}

pub enum PropertyDeclaration<B: Backend> {
    Value {
        declaration: Rc<dyn Declaration<B>>,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    },
    Accessor {
        get: Option<FunctionDeclaration<B>>,
        set: Option<FunctionDeclaration<B>>,
        enumerable: bool,
        configurable: bool,
    },
}

impl<B: Backend> PropertyDeclaration<B> {
    pub fn value<D>(declaration: D) -> Self
    where
        D: Declaration<B> + 'static,
    {
        Self::Value {
            declaration: Rc::new(declaration),
            writable: true,
            enumerable: false,
            configurable: true,
        }
    }

    pub fn constant<D>(declaration: D) -> Self
    where
        D: Declaration<B> + 'static,
    {
        Self::Value {
            declaration: Rc::new(declaration),
            writable: false,
            enumerable: false,
            configurable: false,
        }
    }

    pub fn getter(get: FunctionDeclaration<B>) -> Self {
        Self::Accessor {
            get: Some(get),
            set: None,
            enumerable: false,
            configurable: true,
        }
    }

    pub fn setter(set: FunctionDeclaration<B>) -> Self {
        Self::Accessor {
            get: None,
            set: Some(set),
            enumerable: false,
            configurable: true,
        }
    }

    pub fn merge(&self, later: Self) -> Self {
        match (self, later) {
            (
                Self::Accessor { get, set, .. },
                Self::Accessor {
                    get: later_get,
                    set: later_set,
                    enumerable,
                    configurable,
                },
            ) => Self::Accessor {
                get: later_get.or_else(|| get.clone()),
                set: later_set.or_else(|| set.clone()),
                enumerable,
                configurable,
            },
            (_, later) => later,
        }
    }
}

impl<B: BackendCallables> PropertyDeclaration<B> {
    pub fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        key: &PropertyKey,
    ) -> Result<Property<'js, B>, Error> {
        match self {
            Self::Value {
                declaration,
                writable,
                enumerable,
                configurable,
            } => Ok(Property::Value {
                value: declaration.realise(enter, state, &key.function_name())?,
                writable: *writable,
                enumerable: *enumerable,
                configurable: *configurable,
            }),
            Self::Accessor { get, set, enumerable, configurable } => Ok(Property::Accessor {
                get: Self::realise_accessor(
                    enter,
                    state,
                    get.as_ref(),
                    &format!("get {}", key.function_name()),
                )?,
                set: Self::realise_accessor(
                    enter,
                    state,
                    set.as_ref(),
                    &format!("set {}", key.function_name()),
                )?,
                enumerable: *enumerable,
                configurable: *configurable,
            }),
        }
    }

    pub fn realise_accessor<'js>(
        enter: &Enter<'js, B>,
        state: &HostState,
        accessor: Option<&FunctionDeclaration<B>>,
        name: &str,
    ) -> Result<Option<Val<'js, B>>, Error> {
        accessor
            .map(|accessor| accessor.realise(enter, state, name))
            .transpose()
    }
}

impl<B: Backend> Clone for PropertyDeclaration<B> {
    fn clone(&self) -> Self {
        match self {
            Self::Value {
                declaration,
                writable,
                enumerable,
                configurable,
            } => Self::Value {
                declaration: declaration.clone(),
                writable: *writable,
                enumerable: *enumerable,
                configurable: *configurable,
            },
            Self::Accessor { get, set, enumerable, configurable } => Self::Accessor {
                get: get.clone(),
                set: set.clone(),
                enumerable: *enumerable,
                configurable: *configurable,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{Val, tests::Stub, values::WellKnown},
        enter::Enter,
        errors::Error,
        host::{
            declaration::Declaration,
            property::{PropertyDeclaration, PropertyKey},
            state::HostState,
        },
    };

    struct Unit;

    impl Declaration<Stub> for Unit {
        fn realise<'js>(
            &self,
            _enter: &Enter<'js, Stub>,
            _state: &HostState,
            _name: &str,
        ) -> Result<Val<'js, Stub>, Error> {
            Ok(())
        }
    }

    #[test]
    fn a_name_is_its_own_function_name() {
        assert_eq!(PropertyKey::from("count").function_name(), "count");
    }

    #[test]
    fn a_symbol_is_named_like_javascript() {
        assert_eq!(PropertyKey::from(WellKnown::Iterator).function_name(), "[Symbol.iterator]",);
        assert_eq!(
            PropertyKey::from(WellKnown::AsyncIterator).function_name(),
            "[Symbol.asyncIterator]",
        );
    }

    #[test]
    fn borrowed_and_owned_names_are_one_key() {
        assert_eq!(PropertyKey::from("count"), PropertyKey::from("count".to_owned()));
    }

    #[test]
    fn a_value_is_writable_and_configurable() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::value(Unit),
            PropertyDeclaration::Value {
                writable: true,
                enumerable: false,
                configurable: true,
                ..
            },
        ));
    }

    #[test]
    fn a_constant_is_frozen() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::constant(Unit),
            PropertyDeclaration::Value {
                writable: false,
                enumerable: false,
                configurable: false,
                ..
            },
        ));
    }

    #[test]
    fn a_later_value_replaces_an_earlier_one() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::value(Unit).merge(PropertyDeclaration::constant(Unit)),
            PropertyDeclaration::Value { writable: false, .. },
        ));
    }

    #[test]
    fn a_later_value_replaces_an_accessor() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::Accessor {
                get: None,
                set: None,
                enumerable: false,
                configurable: true,
            }
            .merge(PropertyDeclaration::value(Unit)),
            PropertyDeclaration::Value { .. },
        ));
    }

    #[test]
    fn a_later_accessor_replaces_a_value() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::value(Unit).merge(PropertyDeclaration::Accessor {
                get: None,
                set: None,
                enumerable: false,
                configurable: true,
            }),
            PropertyDeclaration::Accessor { .. },
        ));
    }

    #[test]
    fn merged_accessors_take_the_later_flags() {
        assert!(matches!(
            PropertyDeclaration::<Stub>::Accessor {
                get: None,
                set: None,
                enumerable: true,
                configurable: false,
            }
            .merge(PropertyDeclaration::Accessor {
                get: None,
                set: None,
                enumerable: false,
                configurable: true,
            }),
            PropertyDeclaration::Accessor {
                enumerable: false,
                configurable: true,
                ..
            },
        ));
    }
}
