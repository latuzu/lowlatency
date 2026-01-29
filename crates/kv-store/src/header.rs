//! Snapshot file header format.

use crate::{Error, Result, FORMAT_VERSION, MAGIC_NUMBER};
use sha2::{Digest, Sha256};

/// Header size in bytes (4KB aligned for optimal mmap performance).
pub const HEADER_SIZE: usize = 4096;

/// Offset of the checksum field in the header.
pub const CHECKSUM_OFFSET: usize = 64;

/// Size of the SHA256 checksum.
pub const CHECKSUM_SIZE: usize = 32;

/// Snapshot file header.
///
/// Layout (all fields little-endian):
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
/// 0       8     Magic number (0x4B56535331303030 = "KVSS1000")
/// 8       8     Format version (1)
/// 16      8     Record count
/// 24      8     MPHF index size (bytes)
/// 32      8     Data offset (from start of file)
/// 40      8     Build timestamp (Unix epoch seconds)
/// 48      8     Reserved
/// 56      8     Reserved
/// 64      32    SHA256 checksum of data section
/// 96      4000  Reserved (padding to 4KB)
/// ```
#[derive(Debug, Clone)]
pub struct Header {
    /// Magic number for file identification.
    pub magic: u64,
    /// Format version for compatibility checking.
    pub version: u64,
    /// Number of records in the snapshot.
    pub record_count: u64,
    /// Size of the MPHF index in bytes.
    pub mphf_size: u64,
    /// Offset to the data section from start of file.
    pub data_offset: u64,
    /// Build timestamp (Unix epoch seconds).
    pub build_timestamp: u64,
    /// SHA256 checksum of the data section.
    pub checksum: [u8; CHECKSUM_SIZE],
}

impl Header {
    /// Creates a new header with the given parameters.
    pub fn new(record_count: u64, mphf_size: u64, build_timestamp: u64) -> Self {
        // Calculate data offset: header + mphf, aligned to 4KB
        let mphf_end = HEADER_SIZE as u64 + mphf_size;
        let data_offset = align_to_4k(mphf_end);

        Self {
            magic: MAGIC_NUMBER,
            version: FORMAT_VERSION,
            record_count,
            mphf_size,
            data_offset,
            build_timestamp,
            checksum: [0u8; CHECKSUM_SIZE],
        }
    }

    /// Validates the header and returns an error if invalid.
    pub fn validate(&self) -> Result<()> {
        if self.magic != MAGIC_NUMBER {
            return Err(Error::InvalidMagic {
                expected: MAGIC_NUMBER,
                actual: self.magic,
            });
        }

        if self.version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(self.version));
        }

        // Max 10 billion records (sanity check)
        if self.record_count > 10_000_000_000 {
            return Err(Error::TooManyRecords {
                count: self.record_count,
                max: 10_000_000_000,
            });
        }

        Ok(())
    }

    /// Parses a header from a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::FileTooSmall {
                size: bytes.len() as u64,
                minimum: HEADER_SIZE as u64,
            });
        }

        let magic = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let version = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let record_count = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let mphf_size = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let data_offset = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let build_timestamp = u64::from_le_bytes(bytes[40..48].try_into().unwrap());

        let mut checksum = [0u8; CHECKSUM_SIZE];
        checksum.copy_from_slice(&bytes[CHECKSUM_OFFSET..CHECKSUM_OFFSET + CHECKSUM_SIZE]);

        let header = Self {
            magic,
            version,
            record_count,
            mphf_size,
            data_offset,
            build_timestamp,
            checksum,
        };

        header.validate()?;
        Ok(header)
    }

    /// Serializes the header to a byte array.
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];

        bytes[0..8].copy_from_slice(&self.magic.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.version.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.record_count.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.mphf_size.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.data_offset.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.build_timestamp.to_le_bytes());
        bytes[CHECKSUM_OFFSET..CHECKSUM_OFFSET + CHECKSUM_SIZE].copy_from_slice(&self.checksum);

        bytes
    }

    /// Computes the SHA256 checksum of the data section.
    pub fn compute_checksum(data: &[u8]) -> [u8; CHECKSUM_SIZE] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Verifies the checksum against the data section.
    pub fn verify_checksum(&self, data: &[u8]) -> Result<()> {
        let computed = Self::compute_checksum(data);
        if computed != self.checksum {
            return Err(Error::ChecksumMismatch {
                expected: hex_encode(&self.checksum),
                actual: hex_encode(&computed),
            });
        }
        Ok(())
    }
}

/// Aligns a value up to the next 4KB boundary.
#[inline]
pub fn align_to_4k(value: u64) -> u64 {
    (value + 4095) & !4095
}

/// Hex encode bytes for error messages.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(100_000_000, 30_000_000, 1234567890);
        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.magic, header.magic);
        assert_eq!(parsed.version, header.version);
        assert_eq!(parsed.record_count, header.record_count);
        assert_eq!(parsed.mphf_size, header.mphf_size);
        assert_eq!(parsed.build_timestamp, header.build_timestamp);
    }

    #[test]
    fn test_align_to_4k() {
        assert_eq!(align_to_4k(0), 0);
        assert_eq!(align_to_4k(1), 4096);
        assert_eq!(align_to_4k(4096), 4096);
        assert_eq!(align_to_4k(4097), 8192);
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = Header::new(100, 1000, 0);
        header.magic = 0xDEADBEEF;
        let bytes = header.to_bytes();
        let result = Header::from_bytes(&bytes);
        assert!(matches!(result, Err(Error::InvalidMagic { .. })));
    }
}
