//! Fixed-size record layout for key-value pairs.

/// Key size in bytes (64 bytes).
pub const KEY_SIZE: usize = 64;

/// Value size in bytes (256 bytes).
pub const VALUE_SIZE: usize = 256;

/// Total record size in bytes (320 bytes = 64 + 256).
pub const RECORD_SIZE: usize = KEY_SIZE + VALUE_SIZE;

/// A fixed-size record containing a 64-byte key and 256-byte value.
///
/// The record is laid out in memory as:
/// - Bytes 0-63: Key (64 bytes)
/// - Bytes 64-319: Value (256 bytes)
///
/// Using `#[repr(C, packed)]` ensures consistent memory layout across platforms
/// and no padding between fields.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Record {
    key: [u8; KEY_SIZE],
    value: [u8; VALUE_SIZE],
}

impl Record {
    /// Creates a new record from key and value arrays.
    #[inline]
    pub const fn new(key: [u8; KEY_SIZE], value: [u8; VALUE_SIZE]) -> Self {
        Self { key, value }
    }

    /// Returns a reference to the key bytes.
    #[inline]
    pub fn key(&self) -> &[u8; KEY_SIZE] {
        // Safe because we're reading from a packed struct
        // The key field is always aligned at offset 0
        unsafe { &*(&self.key as *const [u8; KEY_SIZE]) }
    }

    /// Returns a reference to the value bytes.
    #[inline]
    pub fn value(&self) -> &[u8; VALUE_SIZE] {
        // Safe because we're reading from a packed struct
        unsafe { &*(&self.value as *const [u8; VALUE_SIZE]) }
    }

    /// Creates a record from a byte slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice is not exactly `RECORD_SIZE` bytes.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> &Self {
        assert_eq!(bytes.len(), RECORD_SIZE, "Invalid record size");
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    /// Converts the record to a byte slice.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, RECORD_SIZE) }
    }

    /// Compares the key with another key for equality.
    ///
    /// This is optimized for the common case where keys differ early.
    #[inline]
    pub fn key_equals(&self, other: &[u8; KEY_SIZE]) -> bool {
        // Use direct pointer comparison for speed
        // The compiler should optimize this to SIMD instructions
        self.key() == other
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Record")
            .field("key", &hex::encode(self.key()))
            .field("value_len", &VALUE_SIZE)
            .finish()
    }
}

// Hex encoding helper for debug output
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().take(16).map(|b| format!("{:02x}", b)).collect::<String>()
            + if bytes.len() > 16 { "..." } else { "" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_size() {
        assert_eq!(std::mem::size_of::<Record>(), RECORD_SIZE);
    }

    #[test]
    fn test_record_layout() {
        let key = [1u8; KEY_SIZE];
        let value = [2u8; VALUE_SIZE];
        let record = Record::new(key, value);

        assert_eq!(record.key(), &key);
        assert_eq!(record.value(), &value);
    }

    #[test]
    fn test_from_bytes() {
        let mut bytes = [0u8; RECORD_SIZE];
        bytes[0..KEY_SIZE].fill(1);
        bytes[KEY_SIZE..RECORD_SIZE].fill(2);

        let record = Record::from_bytes(&bytes);
        assert!(record.key().iter().all(|&b| b == 1));
        assert!(record.value().iter().all(|&b| b == 2));
    }

    #[test]
    fn test_key_equals() {
        let key = [42u8; KEY_SIZE];
        let value = [0u8; VALUE_SIZE];
        let record = Record::new(key, value);

        assert!(record.key_equals(&key));
        assert!(!record.key_equals(&[0u8; KEY_SIZE]));
    }
}
