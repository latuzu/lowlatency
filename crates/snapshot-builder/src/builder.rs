//! Snapshot builder implementation.

use anyhow::{Context, Result};
use kv_store::{Header, Mphf, KEY_SIZE, RECORD_SIZE, VALUE_SIZE};
use memmap2::MmapMut;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

/// Builder for creating KV store snapshots.
pub struct SnapshotBuilder {
    keys: Vec<[u8; KEY_SIZE]>,
    values: Vec<[u8; VALUE_SIZE]>,
}

impl SnapshotBuilder {
    /// Creates a new empty builder.
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Creates a new builder with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            keys: Vec::with_capacity(capacity),
            values: Vec::with_capacity(capacity),
        }
    }

    /// Adds a key-value pair to the builder.
    pub fn add_record(&mut self, key: [u8; KEY_SIZE], value: [u8; VALUE_SIZE]) {
        self.keys.push(key);
        self.values.push(value);
    }

    /// Returns the number of records added.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns true if no records have been added.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Builds the snapshot file.
    pub fn build(self, output_path: &Path) -> Result<()> {
        let record_count = self.keys.len();
        info!(record_count, "Building MPHF index");

        // Build MPHF from keys
        let mphf = Mphf::build(self.keys.iter())
            .context("Failed to build MPHF index")?;

        let mphf_bytes = mphf.serialize()
            .context("Failed to serialize MPHF")?;

        info!(
            mphf_size = mphf_bytes.len(),
            bits_per_key = (mphf_bytes.len() * 8) as f64 / record_count as f64,
            "MPHF index built"
        );

        // Create header
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let header = Header::new(record_count as u64, mphf_bytes.len() as u64, timestamp);

        // Calculate file layout
        let header_size = kv_store::header::HEADER_SIZE;
        let data_offset = header.data_offset as usize;
        let data_size = record_count * RECORD_SIZE;
        let total_size = data_offset + data_size;

        info!(
            total_size,
            data_offset,
            data_size,
            "Creating snapshot file"
        );

        // Create output file with correct size
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(output_path)
            .context("Failed to create output file")?;

        file.set_len(total_size as u64)
            .context("Failed to set file size")?;

        // Memory-map the file for writing
        let mut mmap = unsafe {
            MmapMut::map_mut(&file).context("Failed to mmap output file")?
        };

        // Write MPHF index (after header)
        mmap[header_size..header_size + mphf_bytes.len()].copy_from_slice(&mphf_bytes);

        // Pad between MPHF and data if needed
        if header_size + mphf_bytes.len() < data_offset {
            mmap[header_size + mphf_bytes.len()..data_offset].fill(0);
        }

        info!("Writing records in MPHF order");

        // Write records in MPHF slot order
        for (key, value) in self.keys.iter().zip(self.values.iter()) {
            let slot = mphf.get(key).expect("Key should be in MPHF");
            let offset = data_offset + slot * RECORD_SIZE;

            mmap[offset..offset + KEY_SIZE].copy_from_slice(key);
            mmap[offset + KEY_SIZE..offset + RECORD_SIZE].copy_from_slice(value);
        }

        // Compute checksum of data section
        info!("Computing checksum");
        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(&mmap[data_offset..]);
            let result: [u8; 32] = hasher.finalize().into();
            result
        };

        // Create final header with checksum
        let mut final_header = header;
        final_header.checksum = checksum;

        // Write header
        mmap[0..header_size].copy_from_slice(&final_header.to_bytes());

        // Flush to disk
        mmap.flush().context("Failed to flush mmap")?;

        info!(
            output = %output_path.display(),
            record_count,
            file_size = total_size,
            "Snapshot built successfully"
        );

        Ok(())
    }
}

impl Default for SnapshotBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kv_store::KvStore;
    use tempfile::NamedTempFile;

    #[test]
    fn test_builder_roundtrip() {
        let mut builder = SnapshotBuilder::new();

        // Add some test records
        for i in 0..100u64 {
            let mut key = [0u8; KEY_SIZE];
            let mut value = [0u8; VALUE_SIZE];
            key[0..8].copy_from_slice(&i.to_le_bytes());
            value[0..8].copy_from_slice(&(i * 100).to_le_bytes());
            builder.add_record(key, value);
        }

        // Build snapshot
        let file = NamedTempFile::new().unwrap();
        builder.build(file.path()).unwrap();

        // Verify by opening with KvStore
        let store = KvStore::open(file.path()).unwrap();
        assert_eq!(store.len(), 100);

        // Verify lookups
        for i in 0..100u64 {
            let mut key = [0u8; KEY_SIZE];
            key[0..8].copy_from_slice(&i.to_le_bytes());

            let value = store.get(&key).expect("Key should exist");
            let stored_value = u64::from_le_bytes(value[0..8].try_into().unwrap());
            assert_eq!(stored_value, i * 100);
        }
    }
}
