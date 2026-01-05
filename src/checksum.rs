use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use base64::engine::general_purpose;
use base64::Engine as _;
use glob::glob;
use rayon::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

/// Length of a base64-encoded SHA-256 hash (32 bytes -> 44 base64 characters).
const BASE64_SHA256_LEN: usize = 44;

/// A SHA-256 checksum encoded as base64.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Checksum([u8; BASE64_SHA256_LEN]);

impl Checksum {
    /// Create a [`Checksum`] from a SHA-256 hash.
    fn from_sha256(hash: &sha2::digest::Output<Sha256>) -> Self {
        let mut buf = [0u8; BASE64_SHA256_LEN];
        general_purpose::STANDARD
            .encode_slice(hash, &mut buf)
            .unwrap_or_else(|_| {
                panic!("base64 encoding of SHA-256 always fits in {BASE64_SHA256_LEN} bytes")
            });
        Self(buf)
    }

    /// Generate a [`Checksum`] from `[u8]` data.
    pub fn from_data(data: impl AsRef<[u8]>) -> Self {
        let hash = Sha256::digest(data);
        Self::from_sha256(&hash)
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        io::copy(&mut reader, &mut hasher)?;
        let hash = hasher.finalize();
        Ok(Self::from_sha256(&hash))
    }

    /// Generate a single [`Checksum`] for paths from `patterns`.
    ///
    /// Hashes files in parallel.
    #[allow(dead_code)]
    pub fn from_globs_par<S: AsRef<str> + Ord + Sync>(patterns: &[S]) -> Self {
        let mut sorted_patterns: Vec<_> = patterns.iter().collect();
        sorted_patterns.sort();

        // Get file paths
        let mut all_paths: Vec<PathBuf> = Vec::new();
        for pattern in sorted_patterns {
            let mut paths: Vec<_> = glob(pattern.as_ref())
                .into_iter()
                .flatten()
                .filter_map(|p| p.ok().filter(|p| p.is_file()))
                .collect();
            paths.sort();
            all_paths.extend(paths);
        }

        // Hash files in parallel, then hash the combined hashes
        let mut final_hasher = Sha256::new();
        all_paths
            .par_iter()
            .filter_map(|path| {
                fs::read(path)
                    .ok()
                    .map(|contents| Sha256::digest(&contents))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|hash| final_hasher.update(hash));

        Self::from_sha256(&final_hasher.finalize())
    }

    /// Returns the checksum as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY: Base64 encoding always produces valid ASCII (and thus UTF-8)
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl fmt::Debug for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Checksum").field(&self.as_str()).finish()
    }
}

impl Serialize for Checksum {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Checksum {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ExpectedLen;
        impl serde::de::Expected for ExpectedLen {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "exactly {BASE64_SHA256_LEN} bytes for base64-encoded SHA-256"
                )
            }
        }

        let s = String::deserialize(deserializer)?;
        let bytes = s.as_bytes();
        if bytes.len() != BASE64_SHA256_LEN {
            return Err(serde::de::Error::invalid_length(bytes.len(), &ExpectedLen));
        }
        let mut buf = [0u8; BASE64_SHA256_LEN];
        buf.copy_from_slice(bytes);
        Ok(Self(buf))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    const PATTERNS: &[&str] = &["src/*", "content/*", "templates/**/*"];

    #[test]
    fn bench_from_globs_par() {
        let iterations = 1_000;

        // Warmup
        for _ in 0..10 {
            std::hint::black_box(Checksum::from_globs_par(PATTERNS));
        }

        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(Checksum::from_globs_par(PATTERNS));
        }
        let elapsed = start.elapsed();
        println!(
            "\nfrom_globs_parallel: {iterations} iterations took {elapsed:?} ({:?}/iter)",
            elapsed / iterations
        );
    }
}
