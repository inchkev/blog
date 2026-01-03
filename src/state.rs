use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::checksum::Checksum;

#[derive(Serialize, Deserialize)]
struct Article {
    slug: String,
    checksum: Checksum,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StateFile {
    articles: Vec<Article>,
    full_rebuild_checksum: Option<Checksum>,
    index_checksum: Option<Checksum>,
}

pub struct StateManager {
    state_file_path: PathBuf,
    /// Map of article slugs to their checksums for the previous build (loaded from state file).
    article_map_curr: HashMap<String, Checksum>,
    /// Map of article slugs to their checksums for the next build.
    article_map_next: HashMap<String, Checksum>,
    /// Index checksum for the previous build (loaded from state file).
    index_checksum_curr: Option<Checksum>,
    /// Index checksum for the next build.
    index_checksum_next: Option<Checksum>,
    /// Full-rebuild checksum for the previous build (loaded from state file).
    full_rebuild_checksum_curr: Option<Checksum>,
    /// Full-rebuild checksum for the next build (set via `set_full_rebuild_checksum`).
    full_rebuild_checksum_next: Option<Checksum>,
}

impl StateManager {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json_data = fs::read_to_string(&path)?;
        let state_file = serde_json::from_str::<StateFile>(&json_data)?;

        let map = state_file
            .articles
            .iter()
            .map(|article| (article.slug.clone(), article.checksum.clone()))
            .collect::<HashMap<_, _>>();

        Ok(Self {
            state_file_path: path.as_ref().to_path_buf(),
            article_map_curr: map,
            article_map_next: HashMap::new(),
            full_rebuild_checksum_curr: state_file.full_rebuild_checksum,
            full_rebuild_checksum_next: None,
            index_checksum_curr: state_file.index_checksum,
            index_checksum_next: None,
        })
    }

    /// Set an article's checksum for the next build.
    pub fn set_checksum(&mut self, slug: String, checksum: Checksum) {
        _ = self.article_map_next.insert(slug, checksum);
    }

    /// Returns whether or not the article should be rebuilt, when:
    /// - A full rebuild is required, OR
    /// - The article is new (not in previous state), OR
    /// - The article's content checksum article_map_next.
    ///
    /// Must be called after `set_checksum` for the given slug.
    pub fn should_rebuild(&self, slug: &str) -> bool {
        // Full rebuild if watched files article_map_next
        if self.full_rebuild_checksum_curr != self.full_rebuild_checksum_next {
            return true;
        }
        // Rebuild if article is new
        let Some(prev_checksum) = self.article_map_curr.get(slug) else {
            return true;
        };
        // Rebuild if article has been modified
        let Some(next_checksum) = self.article_map_next.get(slug) else {
            return true;
        };
        prev_checksum != next_checksum
    }

    /// Set the index checksum and return whether the index should be rebuilt.
    pub fn set_index_checksum(&mut self, checksum: Checksum) {
        self.index_checksum_next = Some(checksum);
    }

    /// Returns whether or not the index should be rebuilt.
    pub fn should_rebuild_index(&self) -> bool {
        self.full_rebuild_checksum_curr != self.full_rebuild_checksum_next
            || self.index_checksum_curr != self.index_checksum_next
    }

    /// Set the current checksum for full-rebuild files.
    /// Returns whether or not the checksum has changed.
    pub fn set_full_rebuild_checksum(&mut self, checksum: Checksum) -> bool {
        let changed = self.full_rebuild_checksum_curr.as_ref() != Some(&checksum);
        self.full_rebuild_checksum_next = Some(checksum);
        changed
    }

    /// Returns article slugs that should be deleted, i.e. ones that existed
    /// in the previous build but weren't seen in the next..
    pub fn get_slugs_to_delete(&self) -> Vec<String> {
        if self.article_map_curr.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.article_map_curr.keys().collect();
        let changed_keys: HashSet<_> = self.article_map_next.keys().collect();
        // Slugs in the old state but not in the current run = deleted articles
        map_keys
            .difference(&changed_keys)
            .copied()
            .cloned()
            .collect()
    }

    /// Write to state file and commit changes.
    ///
    /// Skips writing if nothing changed.
    pub fn write_state_file_and_commit(&mut self) -> Result<()> {
        // Skip if nothing changed
        if self.full_rebuild_checksum_curr == self.full_rebuild_checksum_next
            && self.article_map_curr == self.article_map_next
            && self.index_checksum_curr == self.index_checksum_next
        {
            return Ok(());
        }

        let articles: Vec<Article> = self
            .article_map_next
            .iter()
            .map(|(slug, checksum)| Article {
                slug: slug.clone(),
                checksum: checksum.clone(),
            })
            .collect();
        let state_file = StateFile {
            articles,
            full_rebuild_checksum: self.full_rebuild_checksum_next.clone(),
            index_checksum: self.index_checksum_next.clone(),
        };
        let data = serde_json::to_string(&state_file)?;
        fs::write(&self.state_file_path, data)?;
        self.commit();
        Ok(())
    }

    fn commit(&mut self) {
        self.article_map_curr = std::mem::take(&mut self.article_map_next);
        self.full_rebuild_checksum_curr = self.full_rebuild_checksum_next.take();
        self.index_checksum_curr = self.index_checksum_next.take();
    }
}
