use std::{
    boxed::Box,
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
struct Article {
    slug: String,
    checksum: String,
}

#[derive(Serialize, Deserialize)]
struct Articles {
    articles: Vec<Article>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct StateManager {
    articles: Option<Articles>,
    map: HashMap<String, String>,
    changed: HashMap<String, String>,
}

impl StateManager {
    pub fn from_state_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json_data = fs::read_to_string(path)?;
        let articles = serde_json::from_str::<Articles>(&json_data)?;

        let map = articles
            .articles
            .iter()
            .map(|article| (article.slug.clone(), article.checksum.clone()))
            .collect::<HashMap<_, _>>();

        Ok(Self {
            articles: Some(articles),
            map,
            ..Default::default()
        })
    }

    pub fn contents_changed(&self, slug: &str, checksum: &str) -> bool {
        let Some(c) = self.map.get(slug) else {
            return true;
        };
        c != checksum
    }

    pub fn add_or_keep(&mut self, slug: String, checksum: String) {
        _ = self.changed.insert(slug, checksum);
    }

    pub fn get_stale_slugs(&self) -> Vec<String> {
        if self.map.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.map.keys().collect();
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
        let data = serde_json::to_string(&Articles { articles })?;
        fs::write(path, data)?;
        Ok(())
    }
}

pub fn calculate_sha256_hash(content: &str) -> Box<str> {
    let hash_result = Sha256::digest(content);
    // serialize as 44 length Base64 string
    let mut buf = [0u8; 44];
    let _ = general_purpose::STANDARD
        .encode_slice(hash_result, &mut buf)
        .unwrap();
    // SAFETY: Base64 strings are always valid UTF-8
    unsafe {
        let boxed_bytes: Box<[u8]> = Box::from(buf);
        let ptr = Box::into_raw(boxed_bytes) as *mut str;
        Box::from_raw(ptr)
    }
}
