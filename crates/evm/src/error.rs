//! Abstraction over EVM errors.

use core::error::Error;
use revm::context_interface::result::{EVMError, InvalidTransaction};

/// Abstraction over transaction validation error.
pub trait InvalidTxError: Error + Send + Sync + 'static {
    /// Returns whether the error cause by transaction having a nonce lower than expected.
    fn is_nonce_too_low(&self) -> bool;
}

impl InvalidTxError for InvalidTransaction {
    fn is_nonce_too_low(&self) -> bool {
        matches!(self, Self::NonceTooLow { .. })
    }
}

/// Abstraction over errors that can occur during EVM execution.
///
/// It's assumed that errors can occur either because of an invalid transaction, meaning that other
/// transaction might still result in successful execution, or because of a general EVM
/// misconfiguration.
///
/// If caller occurs a error different from [`EvmError::InvalidTransaction`], it should most likely
/// be treated as fatal error flagging some EVM misconfiguration.
pub trait EvmError: Sized + Error + Send + Sync + 'static {
    /// Errors which might occur as a result of an invalid transaction. i.e unrelated to general EVM
    /// configuration.
    type InvalidTransaction: InvalidTxError;

    /// Returns the [`EvmError::InvalidTransaction`] if the error is an invalid transaction error.
    fn as_invalid_tx_err(&self) -> Option<&Self::InvalidTransaction>;

    /// Attempts to convert the error into [`EvmError::InvalidTransaction`].
    fn try_into_invalid_tx_err(self) -> Result<Self::InvalidTransaction, Self>;

    /// Returns `true` if the error is an invalid transaction error.
    fn is_invalid_tx_err(&self) -> bool {
        self.as_invalid_tx_err().is_some()
    }
}

impl<DBError, TxError> EvmError for EVMError<DBError, TxError>
where
    DBError: Error + Send + Sync + 'static,
    TxError: InvalidTxError,
{
    type InvalidTransaction = TxError;

    fn as_invalid_tx_err(&self) -> Option<&Self::InvalidTransaction> {
        match self {
            Self::Transaction(err) => Some(err),
            _ => None,
        }
    }

    fn try_into_invalid_tx_err(self) -> Result<Self::InvalidTransaction, Self> {
        match self {
            Self::Transaction(err) => Ok(err),
            err => Err(err),
        }
    }
}

#[cfg(feature = "op")]
impl InvalidTxError for op_revm::OpTransactionError {
    fn is_nonce_too_low(&self) -> bool {
        matches!(self, Self::Base(tx) if tx.is_nonce_too_low())
    }
}

/// Error type for EvmInternals trait operations.
#[derive(Debug)]
pub struct EvmInternalsError {
    /// The kind of error that occurred.
    pub kind: EvmInternalsErrorKind,
    /// The underlying error.
    pub error: Box<dyn Error + Send + Sync + 'static>,
}

/// The kind of error that can occur during EvmInternals operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvmInternalsErrorKind {
    /// Database error occurred.
    Database,
    /// Journal operation failed.
    Journal,
    /// State access error.
    State,
    /// Invalid operation.
    InvalidOperation,
    /// Other error.
    Other,
}

impl core::fmt::Display for EvmInternalsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.error)
    }
}

impl Error for EvmInternalsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.error.as_ref())
    }
}

impl EvmInternalsError {
    /// Creates a new EvmInternalsError.
    pub fn new(kind: EvmInternalsErrorKind, error: impl Error + Send + Sync + 'static) -> Self {
        Self { kind, error: Box::new(error) }
    }

    /// Creates a database error.
    pub fn database(error: impl Error + Send + Sync + 'static) -> Self {
        Self::new(EvmInternalsErrorKind::Database, error)
    }

    /// Creates a journal error.
    pub fn journal(error: impl Error + Send + Sync + 'static) -> Self {
        Self::new(EvmInternalsErrorKind::Journal, error)
    }

    /// Creates a state error.
    pub fn state(error: impl Error + Send + Sync + 'static) -> Self {
        Self::new(EvmInternalsErrorKind::State, error)
    }

    /// Creates an invalid operation error.
    pub fn invalid_operation(error: impl Error + Send + Sync + 'static) -> Self {
        Self::new(EvmInternalsErrorKind::InvalidOperation, error)
    }

    /// Creates an other error.
    pub fn other(error: impl Error + Send + Sync + 'static) -> Self {
        Self::new(EvmInternalsErrorKind::Other, error)
    }
}
