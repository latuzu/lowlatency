//! Memory-mapped key-value store implementation.

use crate::{Error, Header, Mphf, Record, Result, KEY_SIZE, RECORD_SIZE, VALUE_SIZE};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// A high-performance, read-only key-value store using memory-mapped files.
///
/// # Design
///
/// The store uses a memory-mapped file containing:
/// 1. A header with metadata and checksum
/// 2. A minimal perfect hash function (MPHF) index
/// 3. An array of fixed-size records (key + value)
///
/// Lookups are O(1):
/// 1. Hash the key using MPHF to get a slot index
/// 2. Read the record at that slot
/// 3. Verify the key matches (MPHF may map unknown keys to valid slots)
///
/// # Thread Safety
///
/// The store is safe to share across threads (`Send + Sync`) because:
/// - The memory map is read-only
/// - No mutable state is held
///
/// # Example
///
/// ```no_run
/// use kv_store::KvStore;
/// use std::path::Path;
///
/// let store = KvStore::open(Path::new("/data/snapshot.bin")).unwrap();
/// let key = [0u8; 64];
/// if let Some(value) = store.get(&key) {
///     println!("Found {} bytes", value.len());
/// }
/// ```
pub struct KvStore {
    /// Memory-mapped snapshot file
    mmap: Mmap,
    /// MPHF index for O(1) slot lookup
    mphf: Mphf,
    /// Number of records in the store
    record_count: usize,
    /// Offset to the record array in the mmap
    data_offset: usize,
    /// Snapshot version info for logging/debugging
    version_info: VersionInfo,
}

/// Version information for the loaded snapshot.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Build timestamp of the snapshot
    pub build_timestamp: u64,
    /// Number of records
    pub record_count: u64,
    /// Size of the snapshot file in bytes
    pub file_size: u64,
}

// SAFETY: KvStore is safe to share across threads because:
// - Mmap is read-only and backed by OS page tables (thread-safe)
// - Mphf is immutable after construction
// - All other fields are primitive types
unsafe impl Send for KvStore {}
unsafe impl Sync for KvStore {}

impl KvStore {
    /// Opens a snapshot file and loads it into memory.
    ///
    /// The entire file is memory-mapped, but pages are loaded on-demand.
    /// Use `preload()` to force all pages into RAM before serving traffic.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the snapshot file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The header is invalid
    /// - The checksum doesn't match
    /// - The MPHF index is corrupted
    pub fn open(path: &Path) -> Result<Self> {
        tracing::info!(path = %path.display(), "Opening snapshot");

        let file = File::open(path)?;
        let file_size = file.metadata()?.len();

        // Memory-map the file (read-only)
        let mmap = unsafe { Mmap::map(&file)? };

        // Parse and validate header
        let header = Header::from_bytes(&mmap)?;
        tracing::debug!(?header, "Parsed snapshot header");

        // Parse MPHF index
        let mphf_start = crate::header::HEADER_SIZE;
        let mphf_end = mphf_start + header.mphf_size as usize;
        let mphf = Mphf::deserialize(&mmap[mphf_start..mphf_end])?;
        tracing::debug!(mphf_size = mphf.size_bytes(), "Loaded MPHF index");

        // Verify record count matches MPHF
        if mphf.len() != header.record_count as usize {
            return Err(Error::InvalidMphf(format!(
                "MPHF key count {} doesn't match header record count {}",
                mphf.len(),
                header.record_count
            )));
        }

        let data_offset = header.data_offset as usize;
        let record_count = header.record_count as usize;

        // Verify file is large enough for all records
        let expected_size = data_offset + record_count * RECORD_SIZE;
        if file_size < expected_size as u64 {
            return Err(Error::FileTooSmall {
                size: file_size,
                minimum: expected_size as u64,
            });
        }

        let version_info = VersionInfo {
            build_timestamp: header.build_timestamp,
            record_count: header.record_count,
            file_size,
        };

        tracing::info!(
            record_count,
            file_size,
            build_timestamp = header.build_timestamp,
            "Snapshot loaded successfully"
        );

        Ok(Self {
            mmap,
            mphf,
            record_count,
            data_offset,
            version_info,
        })
    }

    /// Looks up a value by key.
    ///
    /// # Arguments
    ///
    /// * `key` - The 64-byte key to look up
    ///
    /// # Returns
    ///
    /// The 256-byte value if found, or `None` if the key doesn't exist.
    ///
    /// # Performance
    ///
    /// This operation is O(1) and typically completes in < 1μs when data is in RAM.
    #[inline]
    pub fn get(&self, key: &[u8; KEY_SIZE]) -> Option<&[u8; VALUE_SIZE]> {
        // Step 1: Get slot index from MPHF
        let slot = self.mphf.get(key)?;

        // Step 2: Bounds check (should never fail for valid keys)
        if slot >= self.record_count {
            return None;
        }

        // Step 3: Get record at slot
        let record = self.get_record(slot);

        // Step 4: Verify key matches (critical for correctness!)
        // MPHF may map unknown keys to valid slots
        if record.key_equals(key) {
            Some(record.value())
        } else {
            None
        }
    }

    /// Checks if a key exists in the store.
    #[inline]
    pub fn contains(&self, key: &[u8; KEY_SIZE]) -> bool {
        self.get(key).is_some()
    }

    /// Returns the number of records in the store.
    #[inline]
    pub fn len(&self) -> usize {
        self.record_count
    }

    /// Returns true if the store is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.record_count == 0
    }

    /// Returns version information about the loaded snapshot.
    pub fn version_info(&self) -> &VersionInfo {
        &self.version_info
    }

    /// Preloads all pages into RAM to avoid page faults during serving.
    ///
    /// This should be called during warmup before the server starts accepting traffic.
    /// On Linux, this is equivalent to `vmtouch -t`.
    ///
    /// # Returns
    ///
    /// The number of bytes preloaded.
    pub fn preload(&self) -> Result<usize> {
        tracing::info!(size = self.mmap.len(), "Preloading snapshot into RAM");

        // Touch every page to force it into RAM
        // Page size is typically 4KB on Linux
        const PAGE_SIZE: usize = 4096;
        let mut sum: u8 = 0;

        for chunk in self.mmap.chunks(PAGE_SIZE) {
            // Reading one byte per page forces the page into RAM
            sum = sum.wrapping_add(chunk[0]);
        }

        // Prevent the compiler from optimizing away the reads
        std::hint::black_box(sum);

        tracing::info!(
            bytes = self.mmap.len(),
            "Snapshot preloaded into RAM"
        );

        Ok(self.mmap.len())
    }

    /// Gets a record at the given slot index.
    #[inline]
    fn get_record(&self, slot: usize) -> &Record {
        let offset = self.data_offset + slot * RECORD_SIZE;
        Record::from_bytes(&self.mmap[offset..offset + RECORD_SIZE])
    }

    /// Iterates over all key-value pairs in the store.
    ///
    /// Note: The iteration order is determined by the MPHF slot order,
    /// not insertion order or key order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8; KEY_SIZE], &[u8; VALUE_SIZE])> {
        (0..self.record_count).map(move |slot| {
            let record = self.get_record(slot);
            (record.key(), record.value())
        })
    }
}

impl std::fmt::Debug for KvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KvStore")
            .field("record_count", &self.record_count)
            .field("file_size", &self.mmap.len())
            .field("data_offset", &self.data_offset)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::HEADER_SIZE;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_snapshot(record_count: usize) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();

        // Generate test keys and values
        let keys: Vec<[u8; KEY_SIZE]> = (0..record_count as u64)
            .map(|i| {
                let mut key = [0u8; KEY_SIZE];
                key[0..8].copy_from_slice(&i.to_le_bytes());
                key
            })
            .collect();

        let values: Vec<[u8; VALUE_SIZE]> = (0..record_count as u64)
            .map(|i| {
                let mut value = [0u8; VALUE_SIZE];
                value[0..8].copy_from_slice(&(i * 100).to_le_bytes());
                value
            })
            .collect();

        // Build MPHF
        let mphf = Mphf::build(keys.iter()).unwrap();
        let mphf_bytes = mphf.serialize().unwrap();

        // Create header
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let header = Header::new(record_count as u64, mphf_bytes.len() as u64, timestamp);
        let data_offset = header.data_offset as usize;

        // Calculate total file size
        let file_size = data_offset + record_count * RECORD_SIZE;
        file.as_file().set_len(file_size as u64).unwrap();

        // Write header
        file.write_all(&header.to_bytes()).unwrap();

        // Write MPHF
        file.write_all(&mphf_bytes).unwrap();

        // Pad to data offset
        let current_pos = HEADER_SIZE + mphf_bytes.len();
        if current_pos < data_offset {
            file.write_all(&vec![0u8; data_offset - current_pos]).unwrap();
        }

        // Write records in MPHF order
        let mut records = vec![[0u8; RECORD_SIZE]; record_count];
        for (key, value) in keys.iter().zip(values.iter()) {
            let slot = mphf.get(key).unwrap();
            records[slot][0..KEY_SIZE].copy_from_slice(key);
            records[slot][KEY_SIZE..RECORD_SIZE].copy_from_slice(value);
        }
        for record in records {
            file.write_all(&record).unwrap();
        }

        file.flush().unwrap();
        file
    }

    #[test]
    fn test_store_open_and_get() {
        let file = create_test_snapshot(100);
        let store = KvStore::open(file.path()).unwrap();

        assert_eq!(store.len(), 100);

        // Test known keys
        for i in 0..100u64 {
            let mut key = [0u8; KEY_SIZE];
            key[0..8].copy_from_slice(&i.to_le_bytes());

            let value = store.get(&key).expect("Key should exist");

            let expected_value = i * 100;
            let actual_value = u64::from_le_bytes(value[0..8].try_into().unwrap());
            assert_eq!(actual_value, expected_value);
        }

        // Test unknown key
        let mut unknown_key = [0u8; KEY_SIZE];
        unknown_key[0..8].copy_from_slice(&9999u64.to_le_bytes());
        assert!(store.get(&unknown_key).is_none());
    }

    #[test]
    fn test_store_contains() {
        let file = create_test_snapshot(10);
        let store = KvStore::open(file.path()).unwrap();

        let mut key = [0u8; KEY_SIZE];
        key[0..8].copy_from_slice(&5u64.to_le_bytes());
        assert!(store.contains(&key));

        let mut unknown = [0u8; KEY_SIZE];
        unknown[0..8].copy_from_slice(&999u64.to_le_bytes());
        assert!(!store.contains(&unknown));
    }

    #[test]
    fn test_store_iter() {
        let file = create_test_snapshot(10);
        let store = KvStore::open(file.path()).unwrap();

        let count = store.iter().count();
        assert_eq!(count, 10);
    }
}
