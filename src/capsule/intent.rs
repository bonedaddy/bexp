use crate::types::Intent;

/// Detect the intent behind a query using keyword analysis.
pub fn detect_intent(query: &str) -> Intent {
    let lower = query.to_lowercase();

    let debug_keywords = [
        "bug",
        "fix",
        "error",
        "crash",
        "fail",
        "broken",
        "issue",
        "debug",
        "wrong",
        "undefined",
        "null",
        "exception",
        "panic",
        "stack trace",
        "backtrace",
        "not working",
    ];
    let blast_keywords = [
        "impact",
        "affect",
        "depend",
        "change",
        "refactor",
        "rename",
        "delete",
        "remove",
        "deprecate",
        "breaking",
        "blast radius",
        "who uses",
        "what calls",
        "what depends",
    ];
    let modify_keywords = [
        "add",
        "implement",
        "create",
        "update",
        "modify",
        "extend",
        "feature",
        "enhancement",
        "new",
        "build",
        "write",
        "change",
    ];

    let debug_score: usize = debug_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let blast_score: usize = blast_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let modify_score: usize = modify_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();

    // Tie-break priority: Modify > Debug > BlastRadius > Explore
    let max_score = debug_score.max(blast_score).max(modify_score);
    if max_score == 0 {
        Intent::Explore
    } else if modify_score == max_score {
        Intent::Modify
    } else if debug_score == max_score {
        Intent::Debug
    } else {
        Intent::BlastRadius
    }
}

/// Get intent-specific search weights: (bm25, tfidf, centrality, confidence)
pub fn intent_weights(intent: &Intent) -> (f64, f64, f64, f64) {
    match intent {
        Intent::Debug => (0.45, 0.25, 0.15, 0.15),
        Intent::BlastRadius => (0.15, 0.15, 0.45, 0.25),
        Intent::Modify => (0.30, 0.30, 0.20, 0.20),
        Intent::Explore => (0.30, 0.30, 0.25, 0.15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_intent_prefers_debug_when_debug_keywords_dominate() {
        let query = "app crash with stack trace and panic when opening settings";
        assert_eq!(detect_intent(query), Intent::Debug);
    }

    #[test]
    fn detect_intent_prefers_blast_radius_when_dependency_keywords_dominate() {
        let query = "what calls this and what depends on this module after refactor";
        assert_eq!(detect_intent(query), Intent::BlastRadius);
    }

    #[test]
    fn detect_intent_returns_modify_for_change_requests() {
        let query = "add a new feature and implement the endpoint";
        assert_eq!(detect_intent(query), Intent::Modify);
    }

    #[test]
    fn detect_intent_returns_explore_when_no_keywords_match() {
        let query = "show me the repository overview";
        assert_eq!(detect_intent(query), Intent::Explore);
    }

    #[test]
    fn intent_weights_match_expected_profiles() {
        assert_eq!(intent_weights(&Intent::Debug), (0.45, 0.25, 0.15, 0.15));
        assert_eq!(
            intent_weights(&Intent::BlastRadius),
            (0.15, 0.15, 0.45, 0.25)
        );
        assert_eq!(intent_weights(&Intent::Modify), (0.30, 0.30, 0.20, 0.20));
        assert_eq!(intent_weights(&Intent::Explore), (0.30, 0.30, 0.25, 0.15));
    }
}
