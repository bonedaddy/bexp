use tiktoken_rs::cl100k_base;

pub struct TokenCounter {
    bpe: Option<tiktoken_rs::CoreBPE>,
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter {
    pub fn new() -> Self {
        let bpe = match cl100k_base() {
            Ok(b) => Some(b),
            Err(e) => {
                tracing::warn!(
                    "Failed to load cl100k_base tokenizer: {}. Token counts will be estimated.",
                    e
                );
                None
            }
        };
        Self { bpe }
    }

    pub fn count(&self, text: &str) -> usize {
        match &self.bpe {
            Some(bpe) => bpe.encode_ordinary(text).len(),
            // Rough estimate: ~4 chars per token for code
            None => text.len() / 4,
        }
    }
}
