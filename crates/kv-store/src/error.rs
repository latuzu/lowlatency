//! Error types for the kv-store crate.

use thiserror::Error;

/// Result type alias for kv-store operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when working with the KV store.
#[derive(Error, Debug)]
pub enum Error {
    /// IO error occurred during file operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid magic number in snapshot header.
    #[error("Invalid magic number: expected 0x{expected:016X}, got 0x{actual:016X}")]
    InvalidMagic { expected: u64, actual: u64 },

    /// Unsupported snapshot format version.
    #[error("Unsupported format version: {0}")]
    UnsupportedVersion(u64),

    /// Checksum mismatch - snapshot may be corrupted.
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// Snapshot file is too small to contain valid data.
    #[error("Snapshot file too small: {size} bytes, minimum {minimum} bytes required")]
    FileTooSmall { size: u64, minimum: u64 },

    /// MPHF index is corrupted or invalid.
    #[error("Invalid MPHF index: {0}")]
    InvalidMphf(String),

    /// Record count exceeds maximum supported.
    #[error("Record count {count} exceeds maximum {max}")]
    TooManyRecords { count: u64, max: u64 },

    /// Invalid key size.
    #[error("Invalid key size: expected 64 bytes, got {0} bytes")]
    InvalidKeySize(usize),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}
