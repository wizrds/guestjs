use std::any::Any;

/// The error type returned by all guestjs operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An unexpected error with no more specific category.
    #[error("unexpected error: {message}")]
    Unexpected {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// A value thrown by the guest.
    #[error("{0}")]
    Guest(Box<GuestException>),

    /// A value that does not have the expected type.
    #[error("expected {expected}, found {found}")]
    Conversion {
        expected: String,
        found: String,
    },

    /// A value used outside the guest it belongs to.
    #[error("a value was used outside the guest it belongs to")]
    ForeignContext,

    /// A class payload that is already borrowed.
    #[error("{type_name} is already borrowed ({kind})")]
    Borrow {
        kind: BorrowKind,
        type_name: String,
    },

    /// An operation that is not supported.
    #[error("{message}")]
    Unsupported {
        message: String,
    },

    /// Execution stopped by the engine interrupt.
    #[error("execution was interrupted")]
    Interrupted,

    /// Execution stopped because its deadline passed.
    #[error("execution timed out")]
    Timeout,

    /// Execution stopped because it was cancelled.
    #[error("execution was cancelled")]
    Cancelled,

    /// An entry into a guest that is closed.
    #[error("the guest is closed")]
    Closed,

    /// A failure reported by the engine.
    #[error("{message}")]
    Engine {
        message: String,
    },

    #[error("cannot import {name}: {message}")]
    Import {
        name: String,
        message: String,
    },

    #[error("invalid bundle: {message}")]
    Bundle {
        message: String,
    },

    /// An error returned by host code.
    #[error(transparent)]
    Host(Box<dyn std::error::Error>),
}

impl Error {
    /// Creates an [`Error::Unexpected`](crate::errors::Error::Unexpected) with the given message.
    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected { message: message.into(), source: None }
    }

    /// Creates an [`Error::Unexpected`](crate::errors::Error::Unexpected) with the given message and source error.
    pub fn sourced_unexpected(
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Unexpected {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    /// Creates an [`Error::Guest`](crate::errors::Error::Guest) with the given exception.
    pub fn guest(exception: impl Into<Box<GuestException>>) -> Self {
        Self::Guest(exception.into())
    }

    /// Creates an [`Error::Conversion`](crate::errors::Error::Conversion) with the expected and found type names.
    pub fn conversion(
        expected: impl Into<String>,
        found: impl Into<String>,
    ) -> Self {
        Self::Conversion {
            expected: expected.into(),
            found: found.into(),
        }
    }

    /// Creates an [`Error::ForeignContext`](crate::errors::Error::ForeignContext).
    pub fn foreign_context() -> Self {
        Self::ForeignContext
    }

    /// Creates an [`Error::Borrow`](crate::errors::Error::Borrow) for the given borrow kind and type name.
    pub fn borrow(
        kind: impl Into<BorrowKind>,
        type_name: impl Into<String>,
    ) -> Self {
        Self::Borrow {
            kind: kind.into(),
            type_name: type_name.into(),
        }
    }

    /// Creates an [`Error::Unsupported`](crate::errors::Error::Unsupported) with the given message.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported {
            message: message.into(),
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

    /// Creates an [`Error::Closed`](crate::errors::Error::Closed).
    pub fn closed() -> Self {
        Self::Closed
    }

    /// Creates an [`Error::Engine`](crate::errors::Error::Engine) with the given message.
    pub fn engine(message: impl Into<String>) -> Self {
        Self::Engine {
            message: message.into(),
        }
    }

    /// Creates an [`Error::Import`](crate::errors::Error::Import) with the given name and message.
    pub fn import(
        name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Import {
            name: name.into(),
            message: message.into(),
        }
    }

    /// Creates an [`Error::Bundle`](crate::errors::Error::Bundle) with the given message.
    pub fn bundle(message: impl Into<String>) -> Self {
        Self::Bundle {
            message: message.into(),
        }
    }

    /// Creates an [`Error::Host`](crate::errors::Error::Host) wrapping the given error.
    pub fn host(error: impl Into<Box<dyn std::error::Error>>) -> Self {
        Self::Host(error.into())
    }

    /// Returns true if the error is an [`Error::Unexpected`](crate::errors::Error::Unexpected).
    pub fn is_unexpected(&self) -> bool {
        matches!(self, Self::Unexpected { .. })
    }

    /// Returns true if the error is an [`Error::Guest`](crate::errors::Error::Guest).
    pub fn is_guest(&self) -> bool {
        matches!(self, Self::Guest(_))
    }

    /// Returns true if the error is an [`Error::Conversion`](crate::errors::Error::Conversion).
    pub fn is_conversion(&self) -> bool {
        matches!(self, Self::Conversion { .. })
    }

    /// Returns true if the error is an [`Error::ForeignContext`](crate::errors::Error::ForeignContext).
    pub fn is_foreign_context(&self) -> bool {
        matches!(self, Self::ForeignContext)
    }

    /// Returns true if the error is an [`Error::Borrow`](crate::errors::Error::Borrow).
    pub fn is_borrow(&self) -> bool {
        matches!(self, Self::Borrow { .. })
    }

    /// Returns true if the error is an [`Error::Unsupported`](crate::errors::Error::Unsupported).
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }

    /// Returns true if the error is an [`Error::Interrupted`](crate::errors::Error::Interrupted).
    pub fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted)
    }

    /// Returns true if the error is an [`Error::Timeout`](crate::errors::Error::Timeout).
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout)
    }

    /// Returns true if the error is an [`Error::Cancelled`](crate::errors::Error::Cancelled).
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Returns true if the error is an [`Error::Closed`](crate::errors::Error::Closed).
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Returns true if the error is an [`Error::Engine`](crate::errors::Error::Engine).
    pub fn is_engine(&self) -> bool {
        matches!(self, Self::Engine { .. })
    }

    /// Returns true if the error is an [`Error::Import`](crate::errors::Error::Import).
    pub fn is_import(&self) -> bool {
        matches!(self, Self::Import { .. })
    }

    /// Returns true if the error is an [`Error::Bundle`](crate::errors::Error::Bundle).
    pub fn is_bundle(&self) -> bool {
        matches!(self, Self::Bundle { .. })
    }

    /// Returns true if the error is an [`Error::Host`](crate::errors::Error::Host).
    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host(_))
    }

    /// Returns the [`GuestErrorKind`](crate::errors::GuestErrorKind) this error is thrown as in a guest.
    pub fn guest_kind(&self) -> GuestErrorKind {
        match self {
            Error::Conversion { .. }
            | Error::ForeignContext
            | Error::Borrow { .. }
            | Error::Unsupported { .. } => GuestErrorKind::TypeError,
            Error::Import { .. } => GuestErrorKind::ReferenceError,
            Error::Unexpected { .. }
            | Error::Guest(_)
            | Error::Interrupted
            | Error::Timeout
            | Error::Cancelled
            | Error::Closed
            | Error::Engine { .. }
            | Error::Bundle { .. }
            | Error::Host(_) => GuestErrorKind::Error,
        }
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::unexpected(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::unexpected(value)
    }
}

/// A value thrown by the guest, with its name, message, stack and prototype names.
pub struct GuestException {
    name: String,
    message: String,
    stack: Option<String>,
    prototypes: Vec<String>,
    value: Box<dyn Any>,
}

impl GuestException {
    /// Creates a [`GuestException`](crate::errors::GuestException) that holds the thrown value.
    pub fn new<O: 'static>(
        name: String,
        message: String,
        stack: Option<String>,
        prototypes: Vec<String>,
        value: O,
    ) -> Self {
        Self {
            name,
            message,
            stack,
            prototypes,
            value: Box::new(value),
        }
    }

    /// Returns the error name, or the `typeof` of a value that is not an error.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the stack trace, if the value has one.
    pub fn stack(&self) -> Option<&str> {
        self.stack.as_deref()
    }

    /// Returns true if a constructor on the prototype chain of the value has the given name.
    pub fn matches(&self, name: &str) -> bool {
        self.prototypes
            .iter()
            .any(|prototype| prototype == name)
    }

    /// Returns the thrown value if it has type `O`.
    pub fn value<O: 'static>(&self) -> Option<&O> {
        self.value.downcast_ref()
    }
}

impl std::fmt::Display for GuestException {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match (self.name.is_empty(), self.message.is_empty()) {
            (_, true) => write!(formatter, "{}", self.name),
            (true, false) => write!(formatter, "{}", self.message),
            (false, false) => write!(formatter, "{}: {}", self.name, self.message),
        }
    }
}

impl std::fmt::Debug for GuestException {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("GuestException")
            .field("name", &self.name)
            .field("message", &self.message)
            .field("stack", &self.stack)
            .field("prototypes", &self.prototypes)
            .finish_non_exhaustive()
    }
}

/// The kind of borrow that is already held.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BorrowKind {
    /// A shared borrow.
    Shared,
    /// An exclusive borrow.
    Exclusive,
}

impl std::fmt::Display for BorrowKind {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(match self {
            BorrowKind::Shared => "shared",
            BorrowKind::Exclusive => "exclusive",
        })
    }
}

/// The JavaScript error class a host error is thrown as in a guest.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GuestErrorKind {
    /// A JavaScript `Error`.
    Error,
    /// A JavaScript `TypeError`.
    TypeError,
    /// A JavaScript `RangeError`.
    RangeError,
    /// A JavaScript `ReferenceError`.
    ReferenceError,
    /// A JavaScript `SyntaxError`.
    SyntaxError,
}

#[cfg(test)]
mod tests {
    use crate::errors::{
        BorrowKind,
        Error,
        GuestErrorKind,
    };

    #[test]
    fn borrow_error_names_the_held_borrow() {
        assert_eq!(
            Error::borrow(BorrowKind::Shared, "Counter").to_string(),
            "Counter is already borrowed (shared)",
        );
    }

    #[test]
    fn closed_is_thrown_as_error() {
        assert_eq!(Error::closed().guest_kind(), GuestErrorKind::Error);
        assert_eq!(Error::closed().to_string(), "the guest is closed");
    }

    #[test]
    fn policy_errors_are_thrown_as_error() {
        assert_eq!(Error::timeout().guest_kind(), GuestErrorKind::Error);
        assert_eq!(Error::cancelled().guest_kind(), GuestErrorKind::Error);
        assert_eq!(Error::timeout().to_string(), "execution timed out");
        assert_eq!(Error::cancelled().to_string(), "execution was cancelled");
    }
}
