use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::Result;
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

        let mut map = HashMap::new();
        for article in articles.articles.iter() {
            map.insert(article.slug.clone(), article.checksum.clone());
        }

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

    pub fn add_or_keep(&mut self, slug: &str, checksum: &str) {
        _ = self.changed.insert(slug.to_owned(), checksum.to_owned())
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
        let mut articles = vec![];
        for (slug, checksum) in self.changed.iter() {
            articles.push(Article {
                slug: slug.clone(),
                checksum: checksum.clone(),
            });
        }
        let data = serde_json::to_string(&Articles { articles })?;
        fs::write(path, data)?;
        Ok(())
    }
}

pub fn calculate_sha256_hash(content: &str) -> Result<String> {
    let hash = Sha256::digest(content);

    // serialize as 64-length hex string
    let mut hex_hash_buf = [0u8; 64];
    let hex_hash = base16ct::lower::encode_str(&hash, &mut hex_hash_buf).unwrap();
    Ok(hex_hash.to_string())
}
