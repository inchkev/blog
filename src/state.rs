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
}

pub struct StateManager {
    state_file_path: PathBuf,
    /// Map of article slugs to their checksums for the previous build (loaded from state file).
    curr_article_map: HashMap<String, Checksum>,
    /// Map of article slugs to their checksums for the next build.
    next_article_map: HashMap<String, Checksum>,
    /// Full-rebuild checksum for the previous build (loaded from state file).
    curr_full_rebuild_checksum: Option<Checksum>,
    /// Full-rebuild checksum for the next build (set via `set_full_rebuild_checksum`).
    next_full_rebuild_checksum: Option<Checksum>,
}

impl StateManager {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json_data = fs::read_to_string(&path)?;
        let state_file = serde_json::from_str::<StateFile>(&json_data)?;

        let map = state_file
            .articles
            .iter()
            .map(|article| (article.slug.clone(), article.checksum.clone()))
            .collect::<HashMap<_, _>>();

        Ok(Self {
            state_file_path: path.as_ref().to_path_buf(),
            curr_article_map: map,
            next_article_map: HashMap::new(),
            curr_full_rebuild_checksum: state_file.full_rebuild_checksum,
            next_full_rebuild_checksum: None,
        })
    }

    /// Set the current checksum for full-rebuild files.
    /// Returns whether or not the checksum has changed.
    pub fn set_full_rebuild_checksum(&mut self, checksum: Checksum) -> bool {
        let changed = self.curr_full_rebuild_checksum.as_ref() != Some(&checksum);
        self.next_full_rebuild_checksum = Some(checksum);
        changed
    }

    /// Register an article's checksum for the current build.
    pub fn set_checksum(&mut self, slug: String, checksum: Checksum) {
        _ = self.next_article_map.insert(slug, checksum);
    }

    /// Returns whether or not the article should be rebuilt, when:
    /// - A full rebuild is required, OR
    /// - The article is new (not in previous state), OR
    /// - The article's content checksum next_article_map.
    ///
    /// Must be called after `set_checksum` for the given slug.
    pub fn should_rebuild(&self, slug: &str) -> bool {
        // Full rebuild if watched files next_article_map
        if self.curr_full_rebuild_checksum != self.next_full_rebuild_checksum {
            return true;
        }
        // Rebuild if article is new
        let Some(prev_checksum) = self.curr_article_map.get(slug) else {
            return true;
        };
        // Rebuild if article has been modified
        let Some(next_checksum) = self.next_article_map.get(slug) else {
            return true;
        };
        prev_checksum != next_checksum
    }

    /// Returns article slugs that should be deleted, i.e. ones that existed
    /// in the previous build but weren't seen in the next..
    pub fn get_slugs_to_delete(&self) -> Vec<String> {
        if self.curr_article_map.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.curr_article_map.keys().collect();
        let changed_keys: HashSet<_> = self.next_article_map.keys().collect();
        // Slugs in the old state but not in the current run = deleted articles
        map_keys
            .difference(&changed_keys)
            .copied()
            .cloned()
            .collect()
    }

    /// Write to state file and commit changes.
    ///
    /// Skips writing if nothing next_article_map.
    pub fn write_state_file_and_commit(&mut self) -> Result<()> {
        // Skip if nothing next_article_map
        if self.curr_full_rebuild_checksum == self.next_full_rebuild_checksum
            && self.curr_article_map == self.next_article_map
        {
            return Ok(());
        }

        let articles: Vec<Article> = self
            .next_article_map
            .iter()
            .map(|(slug, checksum)| Article {
                slug: slug.clone(),
                checksum: checksum.clone(),
            })
            .collect();
        let state_file = StateFile {
            articles,
            full_rebuild_checksum: self.next_full_rebuild_checksum.clone(),
        };
        let data = serde_json::to_string(&state_file)?;
        fs::write(&self.state_file_path, data)?;
        self.commit();
        Ok(())
    }

    fn commit(&mut self) {
        self.curr_article_map = std::mem::take(&mut self.next_article_map);
        self.curr_full_rebuild_checksum = self.next_full_rebuild_checksum.take();
    }
}
