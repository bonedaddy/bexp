pub mod languages;
pub mod token_counter;
pub mod transformer;

use std::path::Path;
use std::sync::Arc;

use crate::db::{queries, Database};
use crate::error::{BexpError, Result};
use crate::types::{DetailLevel, Language};

use self::token_counter::TokenCounter;
use self::transformer::SkeletonTransformer;

pub struct Skeletonizer {
    db: Arc<Database>,
    token_counter: TokenCounter,
}

impl Skeletonizer {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            token_counter: TokenCounter::new(),
        }
    }

    pub fn skeletonize(&self, file_path: &Path, level: DetailLevel) -> Result<String> {
        tracing::trace!(path = %file_path.display(), level = ?level, "Skeletonize requested");
        // Check cache first
        let rel_path = file_path.to_string_lossy().to_string();

        {
            let conn = self.db.reader()?;
            if let Ok(Some(file)) = queries::get_file_by_path(&conn, &rel_path) {
                let cached = match level {
                    DetailLevel::Minimal => conn
                        .query_row(
                            "SELECT skeleton_minimal FROM files WHERE id = ?1",
                            rusqlite::params![file.id],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten(),
                    DetailLevel::Standard => conn
                        .query_row(
                            "SELECT skeleton_standard FROM files WHERE id = ?1",
                            rusqlite::params![file.id],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten(),
                    DetailLevel::Detailed => conn
                        .query_row(
                            "SELECT skeleton_detailed FROM files WHERE id = ?1",
                            rusqlite::params![file.id],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten(),
                };

                if let Some(skeleton) = cached {
                    tracing::trace!(path = %file_path.display(), "Skeleton cache hit");
                    return Ok(skeleton);
                }
            }
        }

        tracing::trace!(path = %file_path.display(), "Skeleton cache miss, generating");

        // Generate skeleton
        let source = std::fs::read_to_string(file_path)
            .map_err(|e| BexpError::Skeleton(format!("Cannot read file: {e}")))?;

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = Language::from_extension(ext).ok_or_else(|| BexpError::UnsupportedLanguage {
            extension: ext.to_string(),
        })?;

        let skeleton = SkeletonTransformer::transform(&source, lang, level)?;

        // Cache it — use writer for both lookup and update to avoid TOCTOU
        // where the file row could be deleted by another writer between lookup and update.
        {
            let conn = self.db.writer()?;
            if let Ok(Some(file)) = queries::get_file_by_path(&conn, &rel_path) {
                let tokens = self.token_counter.count(&skeleton) as i64;
                if let Err(e) =
                    queries::update_file_skeleton(&conn, file.id, level.as_str(), &skeleton, tokens)
                {
                    tracing::error!(
                        file_id = file.id,
                        path = %rel_path,
                        error = %e,
                        "Failed to cache skeleton; will regenerate on next request"
                    );
                }
            }
        }

        Ok(skeleton)
    }

    /// Pre-warm skeleton cache for all files missing a cached skeleton at the given level.
    /// Uses writer connection for both read and write to avoid deadlocks.
    pub fn prewarm_skeletons(&self, level: DetailLevel) -> Result<usize> {
        let conn = self.db.writer()?;
        let files = queries::get_files_missing_skeleton(&conn, level.as_str())?;
        let total = files.len();
        if total == 0 {
            return Ok(0);
        }

        tracing::info!(count = total, level = ?level, "Pre-warming skeleton cache");
        let mut cached = 0;
        for file in &files {
            let ext = std::path::Path::new(&file.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let lang = match Language::from_extension(ext) {
                Some(l) => l,
                None => continue,
            };
            let source = match std::fs::read_to_string(&file.path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let skeleton = match SkeletonTransformer::transform(&source, lang, level) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let tokens = self.token_counter.count(&skeleton) as i64;
            if queries::update_file_skeleton(&conn, file.id, level.as_str(), &skeleton, tokens)
                .is_ok()
            {
                cached += 1;
            }
        }
        tracing::info!(cached = cached, total = total, level = ?level, "Skeleton pre-warm complete");
        Ok(cached)
    }

    pub fn skeletonize_source(
        &self,
        source: &str,
        lang: Language,
        level: DetailLevel,
    ) -> Result<String> {
        SkeletonTransformer::transform(source, lang, level)
    }

    #[allow(dead_code)]
    pub fn count_tokens(&self, text: &str) -> usize {
        self.token_counter.count(text)
    }

    /// Fast approximate token count for budget allocation decisions.
    pub fn count_tokens_fast(&self, text: &str) -> usize {
        self.token_counter.count_fast(text)
    }
}
