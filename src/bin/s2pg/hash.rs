//! Deterministic digests shared by `snapshot`, `load` and `verify`.
//!
//! The snapshot identifier is the sha256 of the snapshot file. Table fingerprints use
//! chunked sha256 over canonically encoded primary key tuples, and text columns use an
//! order-independent additive sha256 over individual values.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::types::SqlValue;

/// Number of primary key tuples folded into one chunk digest.
const CHUNK_ROWS: usize = 1024;

/// Returns `(byte_length, sha256_hex)` for a file, read in bounded chunks.
pub fn sha256_file(path: &Path) -> Result<(u64, String)> {
    let file =
        File::open(path).map_err(|err| Error::msg(format!("open {}: {err}", path.display())))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| Error::msg(format!("read {}: {err}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((total, to_hex(&hasher.finalize())))
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

/// Chunked digest over ordered rows: every chunk of [`CHUNK_ROWS`] encoded rows is hashed,
/// then the chunk digests are hashed into the final value.
#[derive(Debug)]
pub struct ChunkedDigest {
    outer: Sha256,
    chunk: Sha256,
    chunk_rows: usize,
}

impl ChunkedDigest {
    pub fn new() -> Self {
        Self {
            outer: Sha256::new(),
            chunk: Sha256::new(),
            chunk_rows: 0,
        }
    }

    pub fn push_row(&mut self, encoded: &[u8]) {
        self.chunk.update((encoded.len() as u64).to_be_bytes());
        self.chunk.update(encoded);
        self.chunk_rows += 1;
        if self.chunk_rows == CHUNK_ROWS {
            self.flush_chunk();
        }
    }

    pub fn finish(mut self) -> String {
        self.flush_chunk();
        to_hex(&self.outer.finalize())
    }

    fn flush_chunk(&mut self) {
        if self.chunk_rows == 0 {
            return;
        }
        let digest = std::mem::replace(&mut self.chunk, Sha256::new()).finalize();
        self.outer.update((self.chunk_rows as u64).to_be_bytes());
        self.outer.update(digest);
        self.chunk_rows = 0;
    }
}

impl Default for ChunkedDigest {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonical, type-tagged encoding of one primary key component. Both sides encode the same
/// logical value to the same bytes, independent of the database that produced it.
pub fn encode_primary_key_value(value: &SqlValue) -> Vec<u8> {
    let mut encoded = Vec::new();
    match value {
        SqlValue::Null => encoded.push(b'n'),
        SqlValue::Integer(raw) => {
            encoded.push(b'i');
            encoded.extend_from_slice(&raw.to_be_bytes());
        }
        SqlValue::Real(raw) => {
            encoded.push(b'd');
            encoded.extend_from_slice(&raw.to_bits().to_be_bytes());
        }
        SqlValue::Text(raw) => {
            encoded.push(b't');
            encoded.extend_from_slice(&(raw.len() as u64).to_be_bytes());
            encoded.extend_from_slice(raw.as_bytes());
        }
        SqlValue::Bytes(raw) => {
            encoded.push(b'b');
            encoded.extend_from_slice(&(raw.len() as u64).to_be_bytes());
            encoded.extend_from_slice(raw);
        }
    }
    encoded
}

/// Order-independent additive sha256. Duplicate values are counted because the 256-bit
/// digest is added as an unsigned integer instead of being XORed.
#[derive(Debug, Clone)]
pub struct AdditiveDigest([u8; 32]);

impl AdditiveDigest {
    pub fn new() -> Self {
        Self([0u8; 32])
    }

    pub fn add_value(&mut self, value: &[u8]) {
        self.add_digest(Sha256::digest(value).into());
    }

    pub fn hex(&self) -> String {
        to_hex(&self.0)
    }

    fn add_digest(&mut self, digest: [u8; 32]) {
        let mut carry = 0u16;
        for index in (0..32).rev() {
            let sum = u16::from(self.0[index]) + u16::from(digest[index]) + carry;
            self.0[index] = sum as u8;
            carry = sum >> 8;
        }
    }
}

impl Default for AdditiveDigest {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{AdditiveDigest, ChunkedDigest, encode_primary_key_value};
    use crate::types::SqlValue;

    #[test]
    fn chunked_digest_is_order_sensitive_and_stable() {
        let mut first = ChunkedDigest::new();
        let mut second = ChunkedDigest::new();
        let mut reversed = ChunkedDigest::new();
        for value in [1i64, 2, 3000] {
            first.push_row(&encode_primary_key_value(&SqlValue::Integer(value)));
            second.push_row(&encode_primary_key_value(&SqlValue::Integer(value)));
        }
        for value in [3000i64, 2, 1] {
            reversed.push_row(&encode_primary_key_value(&SqlValue::Integer(value)));
        }
        assert_eq!(first.finish(), second.finish());
        let mut third = ChunkedDigest::new();
        for value in [1i64, 2, 3000] {
            third.push_row(&encode_primary_key_value(&SqlValue::Integer(value)));
        }
        assert_ne!(third.finish(), reversed.finish());
    }

    #[test]
    fn additive_digest_counts_duplicates() {
        let mut once = AdditiveDigest::new();
        once.add_value(b"alpha");
        once.add_value(b"beta");
        let mut twice = AdditiveDigest::new();
        twice.add_value(b"alpha");
        twice.add_value(b"beta");
        twice.add_value(b"alpha");
        assert_ne!(once.hex(), twice.hex());
    }

    #[test]
    fn encodes_types_with_tags() {
        assert_eq!(
            encode_primary_key_value(&SqlValue::Text("ab".to_string())),
            {
                let mut encoded = vec![b't'];
                encoded.extend_from_slice(&2u64.to_be_bytes());
                encoded.extend_from_slice(b"ab");
                encoded
            }
        );
    }
}
