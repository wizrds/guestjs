use crate::{
    backend::{
        values::{
            BackendValues,
            Kind,
        },
        Val,
    },
    enter::Enter,
    errors::Error,
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
    },
};

pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

macro_rules! number {
    ($($type:ty),+) => {
        $(
            impl<B: BackendValues> ToGuest<B> for $type {
                fn to_guest<'js>(
                    self,
                    enter: &Enter<'js, B>,
                ) -> Result<Val<'js, B>, Error> {
                    f64::from(self).to_guest(enter)
                }
            }
        )+
    };
}

macro_rules! safe_integer {
    ($($type:ty),+) => {
        $(
            impl<B: BackendValues> ToGuest<B> for $type {
                fn to_guest<'js>(
                    self,
                    enter: &Enter<'js, B>,
                ) -> Result<Val<'js, B>, Error> {
                    Some(self as f64)
                        .filter(|number| number.abs() <= MAX_SAFE_INTEGER)
                        .ok_or_else(|| {
                            Error::conversion(
                                "safe integer",
                                self.to_string(),
                            )
                        })
                        .and_then(|number| number.to_guest(enter))
                }
            }
        )+
    };
}

macro_rules! integer {
    ($($type:ty),+) => {
        $(
            impl Describe for $type {
                fn describe(expected: &mut Expected) {
                    i64::describe(expected);
                }
            }

            impl<B: BackendValues> FromGuest<B> for $type {
                type Owned = Self;

                fn from_guest<'js>(
                    enter: &Enter<'js, B>,
                    value: Val<'js, B>,
                ) -> Result<Self::Owned, Error> {
                    i64::from_guest(enter, value).and_then(|integer| {
                        Self::try_from(integer).map_err(|_| {
                            Error::conversion(
                                stringify!($type),
                                integer.to_string(),
                            )
                        })
                    })
                }
            }
        )+
    };
}

number!(i8, i16, i32, u8, u16, u32, f32);
safe_integer!(i64, isize, u64, usize);
integer!(i8, i16, i32, isize, u8, u16, u32, u64, usize);

impl Describe for () {
    fn describe(expected: &mut Expected) {
        expected.push("undefined");
    }
}

impl<B: BackendValues> ToGuest<B> for () {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        Ok(B::undefined(enter.token()))
    }
}

impl<B: BackendValues> FromGuest<B> for () {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Undefined => Ok(()),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl Describe for bool {
    fn describe(expected: &mut Expected) {
        expected.push("boolean");
    }
}

impl<B: BackendValues> ToGuest<B> for bool {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        Ok(B::boolean(enter.token(), self))
    }
}

impl<B: BackendValues> FromGuest<B> for bool {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Boolean => B::as_bool(enter.token(), &value),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl Describe for i64 {
    fn describe(expected: &mut Expected) {
        expected.push("number");
        expected.push("bigint");
    }
}

impl<B: BackendValues> FromGuest<B> for i64 {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Number | Kind::BigInt => B::as_i64(enter.token(), &value),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl Describe for f64 {
    fn describe(expected: &mut Expected) {
        expected.push("number");
    }
}

impl<B: BackendValues> ToGuest<B> for f64 {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        Ok(B::number(enter.token(), self))
    }
}

impl<B: BackendValues> FromGuest<B> for f64 {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Number => B::as_f64(enter.token(), &value),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl Describe for f32 {
    fn describe(expected: &mut Expected) {
        f64::describe(expected);
    }
}

impl<B: BackendValues> FromGuest<B> for f32 {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        f64::from_guest(enter, value).map(|number| number as f32)
    }
}

impl Describe for String {
    fn describe(expected: &mut Expected) {
        expected.push("string");
    }
}

impl<B: BackendValues> ToGuest<B> for &str {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        B::string(enter.token(), self)
    }
}

impl<B: BackendValues> ToGuest<B> for String {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.as_str().to_guest(enter)
    }
}

impl<B: BackendValues> FromGuest<B> for String {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::String => B::as_string(enter.token(), &value),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl Describe for char {
    fn describe(expected: &mut Expected) {
        String::describe(expected);
    }
}

impl<B: BackendValues> ToGuest<B> for char {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.encode_utf8(&mut [0; 4]).to_guest(enter)
    }
}

impl<B: BackendValues> FromGuest<B> for char {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        String::from_guest(enter, value).and_then(|string| {
            string.parse().map_err(|_| {
                Error::conversion(
                    "single-character string",
                    format!("{string:?}"),
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::marshal::describe::Expected;

    #[test]
    fn integers_accept_numbers_and_bigints() {
        assert_eq!(Expected::of::<i64>().to_string(), "number or bigint");
        assert_eq!(Expected::of::<u8>().to_string(), "number or bigint");
    }

    #[test]
    fn floats_accept_numbers() {
        assert_eq!(Expected::of::<f32>().to_string(), "number");
    }

    #[test]
    fn char_accepts_a_string() {
        assert_eq!(Expected::of::<char>().to_string(), "string");
    }

    #[test]
    fn unit_accepts_undefined() {
        assert_eq!(Expected::of::<()>().to_string(), "undefined");
    }
}
