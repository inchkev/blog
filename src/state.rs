use std::boxed::Box;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use base64::engine::general_purpose;
use base64::Engine as _;
use glob::glob;
use serde::{Deserialize, Serialize};
use sha2::digest::Output;
use sha2::{Digest, Sha256};

type Slug = String;
type Checksum = String;

#[derive(Serialize, Deserialize)]
struct Article {
    slug: Slug,
    checksum: Checksum,
}

#[derive(Serialize, Deserialize, Default)]
struct StateFile {
    articles: Vec<Article>,
    #[serde(default, alias = "watched_hash", alias = "watched_checksum")]
    force_rebuild_checksum: Option<Checksum>,
}

pub struct StateManager {
    article_map: HashMap<Slug, Checksum>,
    changed: HashMap<Slug, Checksum>,
    force_rebuild_checksum: Option<Checksum>,
}

impl StateManager {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json_data = fs::read_to_string(path)?;
        let state_file = serde_json::from_str::<StateFile>(&json_data)?;

        let map = state_file
            .articles
            .iter()
            .map(|article| (article.slug.clone(), article.checksum.clone()))
            .collect::<HashMap<_, _>>();

        Ok(Self {
            article_map: map,
            changed: HashMap::new(),
            force_rebuild_checksum: state_file.force_rebuild_checksum,
        })
    }

    pub fn contents_changed(&self, slug: &str, checksum: &str) -> bool {
        let Some(c) = self.article_map.get(slug) else {
            return true;
        };
        c != checksum
    }

    pub fn add_or_keep(&mut self, slug: String, checksum: String) {
        _ = self.changed.insert(slug, checksum);
    }

    pub fn get_stale_slugs(&self) -> Vec<Slug> {
        if self.article_map.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.article_map.keys().collect();
        let changed_keys: HashSet<_> = self.changed.keys().collect();
        map_keys
            .difference(&changed_keys)
            .copied()
            .cloned()
            .collect()
    }

    pub fn write_state_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let articles: Vec<Article> = self
            .changed
            .iter()
            .map(|(slug, checksum)| Article {
                slug: slug.clone(),
                checksum: checksum.clone(),
            })
            .collect();
        let state_file = StateFile {
            articles,
            force_rebuild_checksum: self.force_rebuild_checksum.clone(),
        };
        let data = serde_json::to_string(&state_file)?;
        fs::write(path, data)?;
        Ok(())
    }

    /// Set the current checksum for force-rebuild files and return whether it changed.
    pub fn set_force_rebuild_checksum(&mut self, checksum: Checksum) -> bool {
        let changed = self.force_rebuild_checksum.as_ref() != Some(&checksum);
        self.force_rebuild_checksum = Some(checksum);
        changed
    }
}

pub fn calculate_checksum_str(content: &str) -> Box<str> {
    let hash_result = Sha256::digest(content);
    encode_sha256_as_base64(&hash_result)
}

/// Calculates a checksum for all files matching the given glob patterns.
pub fn calculate_checksum_globs<S: AsRef<str> + Ord>(patterns: &[S]) -> Box<str> {
    let start = Instant::now();

    let mut hasher = Sha256::new();

    let mut sorted_patterns: Vec<_> = patterns.iter().collect();
    sorted_patterns.sort();

    for pattern in sorted_patterns {
        let mut paths: Vec<_> = glob(pattern.as_ref())
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .filter(|p| p.is_file())
            .collect();
        paths.sort();

        for path in paths {
            if let Ok(contents) = fs::read(&path) {
                hasher.update(&contents);
            }
        }
    }

    let hash_result = hasher.finalize();
    println!("calculate_checksum_globs took {:?}", start.elapsed());

    encode_sha256_as_base64(&hash_result)
}

fn encode_sha256_as_base64(hash: &Output<Sha256>) -> Box<str> {
    // SHA256 (32 bytes) encodes to exactly 44 Base64 characters
    let mut buf = [0u8; 44];
    let _ = general_purpose::STANDARD
        .encode_slice(hash, &mut buf)
        .unwrap();
    // SAFETY: Base64 strings are always valid UTF-8
    unsafe {
        let boxed_bytes: Box<[u8]> = Box::from(buf);
        let ptr = Box::into_raw(boxed_bytes) as *mut str;
        Box::from_raw(ptr)
    }
}
