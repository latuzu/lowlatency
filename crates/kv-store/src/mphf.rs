//! Minimal Perfect Hash Function wrapper.
//!
//! Uses the `ph` crate to build and query MPHF indices for O(1) key lookups.

use crate::{Error, Result, KEY_SIZE};
use ph::fmph::{GOFunction, GOBuildConf};
use ph::GetSize;
use std::sync::Arc;

/// Wrapper around the ph crate's MPHF implementation.
///
/// An MPHF maps N keys to exactly N consecutive integers [0, N-1] with no collisions.
/// This enables O(1) lookups with minimal space overhead (~2.5 bits per key).
pub struct Mphf {
    function: Arc<GOFunction>,
    key_count: usize,
}

impl Clone for Mphf {
    fn clone(&self) -> Self {
        Self {
            function: Arc::clone(&self.function),
            key_count: self.key_count,
        }
    }
}

impl Mphf {
    /// Builds a new MPHF from a set of keys.
    ///
    /// # Arguments
    ///
    /// * `keys` - Iterator over 64-byte keys
    ///
    /// # Returns
    ///
    /// A new MPHF that maps each key to a unique index in [0, N-1].
    pub fn build<'a>(keys: impl Iterator<Item = &'a [u8; KEY_SIZE]> + Clone) -> Result<Self> {
        let keys_vec: Vec<_> = keys.clone().map(|k| k.as_slice()).collect();
        let key_count = keys_vec.len();

        if key_count == 0 {
            return Err(Error::InvalidMphf("Cannot build MPHF with zero keys".into()));
        }

        tracing::info!(key_count, "Building MPHF index");

        // Build the MPHF using the GOFunction (Group Optimization) variant
        // which provides good space efficiency (~2.5 bits/key)
        let function = GOFunction::from_slice_with_conf(
            &keys_vec,
            GOBuildConf::default(),
        );

        let size = function.size_bytes();
        tracing::info!(
            key_count,
            size_bytes = size,
            "MPHF index built"
        );

        Ok(Self {
            function: Arc::new(function),
            key_count
        })
    }

    /// Looks up the slot index for a given key.
    ///
    /// # Important
    ///
    /// This function may return a valid index even for keys that were not in the
    /// original keyset. The caller MUST verify that the key at the returned slot
    /// matches the query key.
    ///
    /// # Returns
    ///
    /// The slot index for the key, or `None` if the returned index would be out of bounds.
    #[inline]
    pub fn get(&self, key: &[u8; KEY_SIZE]) -> Option<usize> {
        // The ph crate's get() returns Option<u64>
        let slot = self.function.get(key.as_slice())?;

        // Sanity check: slot should be within bounds
        if (slot as usize) < self.key_count {
            Some(slot as usize)
        } else {
            None
        }
    }

    /// Returns the number of keys in the MPHF.
    #[inline]
    pub fn len(&self) -> usize {
        self.key_count
    }

    /// Returns true if the MPHF is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.key_count == 0
    }

    /// Returns the size of the MPHF in bytes.
    pub fn size_bytes(&self) -> usize {
        self.function.size_bytes()
    }

    /// Serializes the MPHF to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();

        // Write key count
        buf.extend_from_slice(&(self.key_count as u64).to_le_bytes());

        // Serialize the function using the ph crate's write method
        self.function.write(&mut buf)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(buf)
    }

    /// Deserializes an MPHF from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::InvalidMphf("MPHF data too short".into()));
        }

        // Read key count
        let key_count = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;

        // Deserialize function using ph crate's read method
        let function = GOFunction::read(&mut &bytes[8..])
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(Self {
            function: Arc::new(function),
            key_count
        })
    }
}

impl std::fmt::Debug for Mphf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mphf")
            .field("key_count", &self.key_count)
            .field("size_bytes", &self.size_bytes())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(i: u64) -> [u8; KEY_SIZE] {
        let mut key = [0u8; KEY_SIZE];
        key[0..8].copy_from_slice(&i.to_le_bytes());
        key
    }

    #[test]
    fn test_mphf_basic() {
        let keys: Vec<_> = (0..1000u64).map(make_key).collect();
        let mphf = Mphf::build(keys.iter()).unwrap();

        // Check that all keys map to unique slots
        let mut slots: Vec<_> = keys.iter().map(|k| mphf.get(k).unwrap()).collect();
        slots.sort();
        slots.dedup();
        assert_eq!(slots.len(), keys.len());
    }

    #[test]
    fn test_mphf_serialization() {
        let keys: Vec<_> = (0..100u64).map(make_key).collect();
        let mphf = Mphf::build(keys.iter()).unwrap();

        let bytes = mphf.serialize().unwrap();
        let mphf2 = Mphf::deserialize(&bytes).unwrap();

        // Check that deserialized MPHF returns same slots
        for key in &keys {
            assert_eq!(mphf.get(key), mphf2.get(key));
        }
    }

    #[test]
    fn test_mphf_unknown_key() {
        let keys: Vec<_> = (0..100u64).map(make_key).collect();
        let mphf = Mphf::build(keys.iter()).unwrap();

        // Unknown key may return Some(slot), but slot will contain different key
        // This is expected behavior - caller must verify key match
        let unknown = make_key(99999);
        let slot = mphf.get(&unknown);
        // We just verify it doesn't panic; slot may or may not be Some
        let _ = slot;
    }
}
