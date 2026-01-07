use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DefaultOnError};

use crate::checksum::Checksum;

#[serde_as]
#[derive(Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
struct WebsiteState {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(rename = "content")]
    content_map: HashMap<String, Checksum>,
    #[serde(rename = "index")]
    index_checksum: Option<Checksum>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(rename = "bulk")]
    bulk_map: HashMap<PathBuf, FileState>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(rename = "static")]
    static_map: HashMap<PathBuf, FileState>,
}

impl WebsiteState {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        match fs::read_to_string(&path) {
            Ok(json_data) => Ok(serde_json::from_str::<WebsiteState>(&json_data)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(WebsiteState::default()),
            Err(e) => return Err(e.into()),
        }
    }
}

pub struct StateManager {
    state_file_path: PathBuf,
    /// Current state (loaded from state file).
    curr: WebsiteState,
    /// Next state (set).
    next: WebsiteState,
}

impl StateManager {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            state_file_path: path.as_ref().to_path_buf(),
            curr: WebsiteState::from_file(&path)?,
            next: WebsiteState::default(),
        })
    }

    /// Set a page's checksum for the next build.
    pub fn set_checksum(&mut self, slug: String, checksum: Checksum) {
        _ = self.next.content_map.insert(slug, checksum);
    }

    pub fn unset_checksum(&mut self, slug: &str) {
        _ = self.next.content_map.remove(slug);
    }

    /// Returns whether or not the page should be rebuilt, when:
    /// - A full rebuild is required, OR
    /// - The page is new (not in previous state), OR
    /// - The page's content checksum page_map_next.
    ///
    /// Must be called after `set_checksum` for the given slug.
    pub fn should_rebuild(&self, slug: &str) -> bool {
        // Full rebuild if watched files page_map_next
        if self.bulk_has_changed() {
            return true;
        }
        // Rebuild if page is new
        let Some(prev_checksum) = self.curr.content_map.get(slug) else {
            return true;
        };
        // Rebuild if page has been modified
        let Some(next_checksum) = self.next.content_map.get(slug) else {
            return true;
        };
        prev_checksum != next_checksum
    }

    /// Set the index checksum and return whether the index should be rebuilt.
    pub fn set_index_checksum(&mut self, checksum: Checksum) {
        self.next.index_checksum = Some(checksum);
    }

    /// Returns whether or not the index should be rebuilt.
    pub fn should_rebuild_index(&self) -> bool {
        self.bulk_has_changed() || self.curr.index_checksum != self.next.index_checksum
    }

    pub fn bulk_has_changed(&self) -> bool {
        self.curr.bulk_map != self.next.bulk_map
    }

    fn fast_get_new_file_state_and_check_if_changed<P: AsRef<Path>>(
        path: P,
        key: Option<PathBuf>,
        curr_map: &HashMap<PathBuf, FileState>,
    ) -> Result<(FileState, bool)> {
        let mut has_changed = false;
        let path = path.as_ref();
        // fix this key path pathbuf stuff
        let key = key.unwrap_or_else(|| path.to_path_buf());

        let new_file_state = match curr_map.get(&key) {
            Some(curr_file_state) => {
                if curr_file_state.fast_has_changed(path)? {
                    has_changed = true;
                    println!("detected file change (fast): {}", path.display());
                    FileState::from_path(path)?
                } else {
                    *curr_file_state
                }
            }
            None => {
                has_changed = true;
                FileState::from_path(path)?
            }
        };
        Ok((new_file_state, has_changed))
    }

    fn fast_get_new_file_state_map_and_check_if_changed(
        paths: Vec<PathBuf>,
        curr_map: &HashMap<PathBuf, FileState>,
    ) -> Result<(HashMap<PathBuf, FileState>, bool)> {
        let mut has_changed = false;
        let new_map = paths
            .into_iter()
            .filter_map(|path| {
                let (new_file_state, has_changed_) =
                    Self::fast_get_new_file_state_and_check_if_changed(&path, None, curr_map)
                        .ok()?;
                has_changed = has_changed_;
                Some((path, new_file_state))
            })
            .collect::<HashMap<PathBuf, FileState>>();
        Ok((new_map, has_changed))
    }

    pub fn fast_set_next_bulk_and_check_if_changed(&mut self, paths: Vec<PathBuf>) -> Result<bool> {
        let (next_bulk_map, has_changed) =
            Self::fast_get_new_file_state_map_and_check_if_changed(paths, &self.curr.bulk_map)?;
        self.next.bulk_map = next_bulk_map;
        Ok(has_changed)
    }

    pub fn fast_set_next_static_file_state_and_check_if_changed(
        &mut self,
        path: impl AsRef<Path>,
        key: PathBuf,
    ) -> Result<bool> {
        let (new_file_state, has_changed) = Self::fast_get_new_file_state_and_check_if_changed(
            path,
            Some(key.clone()),
            &self.curr.static_map,
        )?;
        self.next.static_map.insert(key, new_file_state);
        Ok(has_changed)
    }

    /// Returns (path, is_file) tuples in descending path depth order, i.e.
    /// the order that they can safely be deleted.
    pub fn get_stale_static_files_in_order_of_deletion(&self) -> Vec<(&PathBuf, bool)> {
        if self.curr.static_map.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.curr.static_map.keys().collect();
        let changed_keys: HashSet<_> = self.next.static_map.keys().collect();
        // Slugs in the old state but not in the current run = deleted pages
        let mut stale_files = map_keys
            .difference(&changed_keys)
            .into_iter()
            .map(|&k| (k, self.curr.static_map.get(k).unwrap().is_file()))
            .collect::<Vec<_>>();
        // Sort by path depth, deepest first
        stale_files.sort_by_key(|(path, _is_file)| Reverse(path.components().count()));
        stale_files
    }

    #[allow(dead_code)]
    pub fn set_bulk(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        let bulk_map = paths
            .par_iter()
            .filter_map(|path| FileState::from_path(path).ok().map(|s| (path.clone(), s)))
            .map(|s| {
                let mut m = HashMap::new();
                m.insert(s.0.clone(), s.1);
                m
            })
            .reduce(
                || HashMap::new(),
                |m1, m2| {
                    m2.iter().fold(m1, |mut acc, (k, vs)| {
                        acc.entry(k.clone()).or_insert(*vs);
                        acc
                    })
                },
            );
        self.next.bulk_map = bulk_map;
        Ok(())
    }

    /// Returns page slugs that should be deleted, i.e. ones that existed
    /// in the previous build but weren't seen in the next..
    pub fn get_slugs_to_delete(&self) -> Vec<String> {
        if self.curr.content_map.is_empty() {
            return vec![];
        }
        let map_keys: HashSet<_> = self.curr.content_map.keys().collect();
        let changed_keys: HashSet<_> = self.next.content_map.keys().collect();
        // Slugs in the old state but not in the current run = deleted pages
        map_keys
            .difference(&changed_keys)
            .copied()
            .cloned()
            .collect()
    }

    /// Write to state file and commit changes.
    ///
    /// Skips writing if nothing changed.
    pub fn write_state_file_and_commit(&mut self, pretty_print: bool) -> Result<()> {
        let data = if pretty_print {
            serde_json::to_string_pretty(&self.next)?
        } else {
            serde_json::to_string(&self.next)?
        };
        fs::write(&self.state_file_path, &data)?;
        println!(
            "WRITE {} ({} bytes)",
            self.state_file_path.display(),
            data.len()
        );
        self.commit();
        Ok(())
    }

    fn commit(&mut self) {
        self.curr = std::mem::take(&mut self.next);
    }
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum FileState {
    /// (mtime, size, checksum)
    File(u64, u64, Checksum),
    Dir,
}

impl PartialEq for FileState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(_, _, checksum), Self::File(_, _, checksum_other)) => {
                checksum == checksum_other
            }
            (Self::Dir, Self::Dir) => true,
            _ => false,
        }
    }
}

impl FileState {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            Ok(Self::Dir)
        } else if metadata.is_file() {
            Ok(Self::File(
                metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs(),
                metadata.len(),
                Checksum::from_file(path)?,
            ))
        } else {
            return Err(anyhow!("expected file or directory"));
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(..))
    }

    #[allow(dead_code)]
    pub fn has_changed<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            Ok(true)
        } else if metadata.is_file() {
            let &Self::File(_, s, c) = self else {
                return Ok(true);
            };
            let size = metadata.len();
            if s != size {
                return Ok(true);
            }
            let checksum = Checksum::from_file(&path)?;
            Ok(c != checksum)
        } else {
            Err(anyhow!("expected file or directory"))
        }
    }

    // TODO: describe the heuristic used to speed up file change checks
    pub fn fast_has_changed<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            // for directories, we're just returning whether/not it exists.
            match self {
                Self::Dir => Ok(false),
                Self::File(..) => Ok(true),
            }
        } else if metadata.is_file() {
            let &Self::File(m, s, c) = self else {
                return Ok(true);
            };
            let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
            let size = metadata.len();
            if m == modified && s == size {
                return Ok(false);
            }
            if s != size {
                return Ok(true);
            }
            let checksum = Checksum::from_file(&path)?;
            Ok(c != checksum)
        } else {
            return Err(anyhow!("expected file or directory"));
        }
    }
}
