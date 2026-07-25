/// The error type returned by all guestjs operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An underlying I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A failure to serialize or deserialize data.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// A failure originating in the JavaScript engine.
    #[error("engine error: {message}")]
    Engine {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An exception thrown by guest code.
    #[error("guest exception: {message}")]
    GuestException {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// A value could not be converted across the host/guest boundary.
    #[error("conversion error: {message}")]
    Conversion {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// A failure to transpile guest source into JavaScript.
    #[error("transpile error: {message}")]
    Transpile {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An interrupted guest execution.
    #[error("execution interrupted")]
    Interrupted,

    /// A guest execution that exceeded its time budget.
    #[error("execution timed out")]
    Timeout,

    /// A cancelled guest execution.
    #[error("execution cancelled")]
    Cancelled,

    /// An unexpected error with no more specific category.
    #[error("unexpected error: {message}")]
    Unexpected {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl Error {
    /// Creates an [`Error::Io`](crate::errors::Error::Io) from the given I/O error.
    pub fn io(error: std::io::Error) -> Self {
        Self::Io(error)
    }

    /// Creates an [`Error::Serialization`](crate::errors::Error::Serialization) with the given serde JSON error.
    pub fn serialization(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }

    /// Creates an [`Error::Engine`](crate::errors::Error::Engine) with the given
    /// message.
    pub fn engine(message: impl Into<String>) -> Self {
        Self::Engine { message: message.into(), source: None }
    }

    /// Creates an [`Error::Engine`](crate::errors::Error::Engine) with the given
    /// message and source error.
    pub fn sourced_engine(
        message: impl Into<String>,
        source: Option<impl Into<Box<dyn std::error::Error + Send + Sync>>>,
    ) -> Self {
        Self::Engine {
            message: message.into(),
            source: source.map(Into::into),
        }
    }

    /// Creates an [`Error::GuestException`](crate::errors::Error::GuestException)
    /// with the given message.
    pub fn guest_exception(message: impl Into<String>) -> Self {
        Self::GuestException { message: message.into(), source: None }
    }

    /// Creates an [`Error::GuestException`](crate::errors::Error::GuestException)
    /// with the given message and source error.
    pub fn sourced_guest_exception(
        message: impl Into<String>,
        source: Option<impl Into<Box<dyn std::error::Error + Send + Sync>>>,
    ) -> Self {
        Self::GuestException {
            message: message.into(),
            source: source.map(Into::into),
        }
    }

    /// Creates an [`Error::Conversion`](crate::errors::Error::Conversion) with the
    /// given message.
    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion { message: message.into(), source: None }
    }

    /// Creates an [`Error::Conversion`](crate::errors::Error::Conversion) with the
    /// given message and source error.
    pub fn sourced_conversion(
        message: impl Into<String>,
        source: Option<impl Into<Box<dyn std::error::Error + Send + Sync>>>,
    ) -> Self {
        Self::Conversion {
            message: message.into(),
            source: source.map(Into::into),
        }
    }

    /// Creates an [`Error::Transpile`](crate::errors::Error::Transpile) with the
    /// given message.
    pub fn transpile(message: impl Into<String>) -> Self {
        Self::Transpile { message: message.into(), source: None }
    }

    /// Creates an [`Error::Transpile`](crate::errors::Error::Transpile) with the
    /// given message and source error.
    pub fn sourced_transpile(
        message: impl Into<String>,
        source: Option<impl Into<Box<dyn std::error::Error + Send + Sync>>>,
    ) -> Self {
        Self::Transpile {
            message: message.into(),
            source: source.map(Into::into),
        }
    }

    /// Creates an [`Error::Interrupted`](crate::errors::Error::Interrupted).
    pub fn interrupted() -> Self {
        Self::Interrupted
    }

    /// Creates an [`Error::Timeout`](crate::errors::Error::Timeout).
    pub fn timeout() -> Self {
        Self::Timeout
    }

    /// Creates an [`Error::Cancelled`](crate::errors::Error::Cancelled).
    pub fn cancelled() -> Self {
        Self::Cancelled
    }

    /// Returns whether this is an [`Error::Interrupted`](crate::errors::Error::Interrupted).
    pub fn is_interrupt(&self) -> bool {
        matches!(self, Self::Interrupted)
    }

    /// Creates an error for building an owned handle on a detached scope.
    pub fn detached_scope() -> Self {
        Self::unexpected("cannot build an owned guest handle on detached scope")
    }

    /// Creates an [`Error::Unexpected`](crate::errors::Error::Unexpected) with the given message.
    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected { message: message.into(), source: None }
    }

    /// Creates an [`Error::Unexpected`](crate::errors::Error::Unexpected) with the given message and source error.
    pub fn sourced_unexpected(
        message: impl Into<String>,
        source: Option<impl Into<Box<dyn std::error::Error + Send + Sync>>>,
    ) -> Self {
        Self::Unexpected {
            message: message.into(),
            source: source.map(Into::into),
        }
    }
}

impl From<rquickjs::CaughtError<'_>> for Error {
    fn from(error: rquickjs::CaughtError<'_>) -> Self {
        match error {
            rquickjs::CaughtError::Error(error) => {
                Self::sourced_engine(error.to_string(), Some(error))
            }
            rquickjs::CaughtError::Exception(error)
                if error
                    .as_value()
                    .is_uncatchable_error() =>
                Self::interrupted(),
            rquickjs::CaughtError::Exception(error) => Self::guest_exception(error.to_string()),
            rquickjs::CaughtError::Value(value) if value.is_uncatchable_error() =>
                Self::interrupted(),
            rquickjs::CaughtError::Value(value) => Self::guest_exception(format!("{value:?}")),
        }
    }
}

impl From<rquickjs::Error> for Error {
    fn from(error: rquickjs::Error) -> Self {
        Self::sourced_engine(error.to_string(), Some(error))
    }
}
