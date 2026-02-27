use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::BexpConfig;
use crate::error::Result;
use crate::types::Language;

pub struct Scanner<'a> {
    config: &'a BexpConfig,
}

impl<'a> Scanner<'a> {
    pub fn new(config: &'a BexpConfig) -> Self {
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
                Err(e) => {
                    tracing::warn!(error = %e, "Skipping entry during scan");
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            // Check for .env files (no extension-based detection)
            if Self::is_dotenv_file(path) {
                if let Ok(metadata) = entry.metadata() {
                    if (metadata.len() as usize) <= self.config.max_file_size {
                        files.push(path.to_path_buf());
                    }
                }
                continue;
            }

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

    /// Check if a file is a .env file (e.g., .env, .env.local, .env.production).
    pub fn is_dotenv_file(path: &Path) -> bool {
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => return false,
        };
        file_name == ".env" || file_name.starts_with(".env.")
    }

    fn is_excluded(&self, path: &Path, root: &Path) -> bool {
        match path.strip_prefix(root) {
            Ok(rel) => self.config.is_excluded(rel),
            Err(_) => {
                tracing::error!(
                    path = %path.display(),
                    root = %root.display(),
                    "Cannot compute relative path; including file to avoid silent exclusion"
                );
                false
            }
        }
    }
}
