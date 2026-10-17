use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NbtError {
    InvalidSyntax(String),
    KeyNotFound(String),
    KeyExists(String),
    PathNotFound(String),
    TypeMismatch(String),
    Locked(String),
    TxNotFound(String),
    TxNotOwner(String),
    TxInvalidState(String),
    CommitFailed(String),
    NbtInvalid(String),
    LimitExceeded(String),
}

impl NbtError {
    pub fn code(&self) -> &'static str {
        match self {
            NbtError::InvalidSyntax(_) => "INVALID_SYNTAX",
            NbtError::KeyNotFound(_) => "KEY_NOT_FOUND",
            NbtError::KeyExists(_) => "KEY_EXISTS",
            NbtError::PathNotFound(_) => "PATH_NOT_FOUND",
            NbtError::TypeMismatch(_) => "TYPE_MISMATCH",
            NbtError::Locked(_) => "LOCKED",
            NbtError::TxNotFound(_) => "TX_NOT_FOUND",
            NbtError::TxNotOwner(_) => "TX_NOT_OWNER",
            NbtError::TxInvalidState(_) => "TX_INVALID_STATE",
            NbtError::CommitFailed(_) => "COMMIT_FAILED",
            NbtError::NbtInvalid(_) => "NBT_INVALID",
            NbtError::LimitExceeded(_) => "LIMIT_EXCEEDED",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            NbtError::InvalidSyntax(m)
            | NbtError::KeyNotFound(m)
            | NbtError::KeyExists(m)
            | NbtError::PathNotFound(m)
            | NbtError::TypeMismatch(m)
            | NbtError::Locked(m)
            | NbtError::TxNotFound(m)
            | NbtError::TxNotOwner(m)
            | NbtError::TxInvalidState(m)
            | NbtError::CommitFailed(m)
            | NbtError::NbtInvalid(m)
            | NbtError::LimitExceeded(m) => m,
        }
    }
}

impl fmt::Display for NbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code(), self.message())
    }
}

impl std::error::Error for NbtError {}

pub type Result<T> = std::result::Result<T, NbtError>;
