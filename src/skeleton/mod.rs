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
            let conn = self.db.reader();
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
        let lang = Language::from_extension(ext)
            .ok_or_else(|| BexpError::UnsupportedLanguage { extension: ext.to_string() })?;

        let skeleton = SkeletonTransformer::transform(&source, lang, level)?;

        // Cache it
        {
            let reader = self.db.reader();
            if let Ok(Some(file)) = queries::get_file_by_path(&reader, &rel_path) {
                let tokens = self.token_counter.count(&skeleton) as i64;
                let conn = self.db.writer();
                let _ = queries::update_file_skeleton(
                    &conn,
                    file.id,
                    level.as_str(),
                    &skeleton,
                    tokens,
                );
            }
        }

        Ok(skeleton)
    }

    pub fn skeletonize_source(
        &self,
        source: &str,
        lang: Language,
        level: DetailLevel,
    ) -> Result<String> {
        SkeletonTransformer::transform(source, lang, level)
    }

    pub fn count_tokens(&self, text: &str) -> usize {
        self.token_counter.count(text)
    }
}
