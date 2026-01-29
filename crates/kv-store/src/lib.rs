//! High-performance in-memory key-value store with MPHF indexing.
//!
//! This crate provides O(1) lookups for fixed-size key-value pairs using
//! Minimal Perfect Hash Functions (MPHF) and memory-mapped files.
//!
//! # Design
//!
//! - Keys: 64 bytes (fixed)
//! - Values: 256 bytes (fixed)
//! - Indexing: MPHF for O(1) slot lookup
//! - Storage: Memory-mapped file for zero-copy access
//!
//! # Example
//!
//! ```no_run
//! use kv_store::KvStore;
//! use std::path::Path;
//!
//! let store = KvStore::open(Path::new("/data/snapshot.bin")).unwrap();
//! let key = [0u8; 64];
//! if let Some(value) = store.get(&key) {
//!     println!("Found value: {} bytes", value.len());
//! }
//! ```

mod error;
pub mod header;
mod mphf;
mod record;
mod store;

pub use error::{Error, Result};
pub use header::{Header, HEADER_SIZE};
pub use mphf::Mphf;
pub use record::{Record, KEY_SIZE, RECORD_SIZE, VALUE_SIZE};
pub use store::KvStore;

/// Snapshot file format version
pub const FORMAT_VERSION: u64 = 1;

/// Magic number for snapshot files: "KVSS1000" in ASCII
pub const MAGIC_NUMBER: u64 = 0x4B56535331303030;
