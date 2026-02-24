use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::VexpConfig;
use crate::error::Result;
use crate::types::Language;

pub struct Scanner<'a> {
    config: &'a VexpConfig,
}

impl<'a> Scanner<'a> {
    pub fn new(config: &'a VexpConfig) -> Self {
        Self { config }
    }

    pub fn scan(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.is_excluded(e.path(), root))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            // Check extension
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };

            if Language::from_extension(ext).is_none() {
                continue;
            }

            // Check file size
            if let Ok(metadata) = entry.metadata() {
                if metadata.len() as usize > self.config.max_file_size {
                    continue;
                }
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    fn is_excluded(&self, path: &Path, root: &Path) -> bool {
        let rel = path.strip_prefix(root).unwrap_or(path);
        self.config.is_excluded(rel)
    }
}
