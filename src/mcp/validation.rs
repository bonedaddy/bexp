use rmcp::model::ErrorData;

const MAX_QUERY_LENGTH: usize = 10_000;
const MAX_CONTENT_LENGTH: usize = 50_000;
const MAX_LIMIT: usize = 1_000;
const MAX_DEPTH: usize = 20;

pub fn validate_query(query: &str) -> Result<(), ErrorData> {
    if query.is_empty() {
        return Err(ErrorData::invalid_params("query must not be empty", None));
    }
    if query.len() > MAX_QUERY_LENGTH {
        return Err(ErrorData::invalid_params(
            format!(
                "query too long: {} chars (max {})",
                query.len(),
                MAX_QUERY_LENGTH
            ),
            None,
        ));
    }
    Ok(())
}

pub fn validate_limit(limit: Option<usize>, default: usize) -> Result<usize, ErrorData> {
    let limit = limit.unwrap_or(default);
    if limit == 0 {
        return Err(ErrorData::invalid_params("limit must be > 0", None));
    }
    if limit > MAX_LIMIT {
        return Err(ErrorData::invalid_params(
            format!("limit too large: {limit} (max {MAX_LIMIT})"),
            None,
        ));
    }
    Ok(limit)
}

pub fn validate_depth(depth: Option<usize>, default: usize) -> Result<usize, ErrorData> {
    let depth = depth.unwrap_or(default);
    if depth == 0 {
        return Err(ErrorData::invalid_params("depth must be > 0", None));
    }
    if depth > MAX_DEPTH {
        return Err(ErrorData::invalid_params(
            format!("depth too large: {depth} (max {MAX_DEPTH})"),
            None,
        ));
    }
    Ok(depth)
}

const VALID_DIRECTIONS: &[&str] = &["callers", "callees", "both"];

pub fn validate_direction(direction: &str) -> Result<(), ErrorData> {
    if !VALID_DIRECTIONS.contains(&direction) {
        return Err(ErrorData::invalid_params(
            format!(
                "invalid direction '{}', must be one of: {}",
                direction,
                VALID_DIRECTIONS.join(", ")
            ),
            None,
        ));
    }
    Ok(())
}

pub fn validate_content(content: &str) -> Result<(), ErrorData> {
    if content.is_empty() {
        return Err(ErrorData::invalid_params("content must not be empty", None));
    }
    if content.len() > MAX_CONTENT_LENGTH {
        return Err(ErrorData::invalid_params(
            format!(
                "content too long: {} chars (max {})",
                content.len(),
                MAX_CONTENT_LENGTH
            ),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_query_rejects_empty() {
        assert!(validate_query("").is_err());
    }

    #[test]
    fn validate_query_accepts_normal() {
        assert!(validate_query("find function foo").is_ok());
    }

    #[test]
    fn validate_query_rejects_too_long() {
        let long = "x".repeat(MAX_QUERY_LENGTH + 1);
        assert!(validate_query(&long).is_err());
    }

    #[test]
    fn validate_limit_uses_default() {
        assert_eq!(validate_limit(None, 50).unwrap(), 50);
    }

    #[test]
    fn validate_limit_rejects_zero() {
        assert!(validate_limit(Some(0), 50).is_err());
    }

    #[test]
    fn validate_limit_rejects_too_large() {
        assert!(validate_limit(Some(1001), 50).is_err());
    }

    #[test]
    fn validate_depth_uses_default() {
        assert_eq!(validate_depth(None, 3).unwrap(), 3);
    }

    #[test]
    fn validate_depth_rejects_too_large() {
        assert!(validate_depth(Some(21), 3).is_err());
    }

    #[test]
    fn validate_direction_accepts_valid() {
        assert!(validate_direction("callers").is_ok());
        assert!(validate_direction("callees").is_ok());
        assert!(validate_direction("both").is_ok());
    }

    #[test]
    fn validate_direction_rejects_invalid() {
        assert!(validate_direction("invalid").is_err());
    }

    #[test]
    fn validate_content_rejects_empty() {
        assert!(validate_content("").is_err());
    }

    #[test]
    fn validate_content_accepts_normal() {
        assert!(validate_content("some observation content").is_ok());
    }
}
