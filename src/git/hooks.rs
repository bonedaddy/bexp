use std::path::Path;

use crate::error::{Result, VexpError};

/// Generate git hooks for vexp integration.
#[allow(dead_code)]
pub fn generate_hooks(workspace_root: &Path, vexp_binary: &str) -> Result<()> {
    let git_dir = workspace_root.join(".git");
    if !git_dir.exists() {
        return Err(VexpError::Config("Not a git repository".to_string()));
    }

    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    // Pre-commit hook: flush WAL
    let pre_commit = hooks_dir.join("pre-commit");
    let pre_commit_content = format!(
        r#"#!/bin/sh
# Vexp: Flush WAL before commit
if command -v {} >/dev/null 2>&1; then
    {} flush-wal 2>/dev/null || true
fi
"#,
        vexp_binary, vexp_binary
    );
    write_hook(&pre_commit, &pre_commit_content)?;

    // Post-merge hook: adopt index
    let post_merge = hooks_dir.join("post-merge");
    let post_merge_content = format!(
        r#"#!/bin/sh
# Vexp: Re-index after merge
if command -v {} >/dev/null 2>&1; then
    {} reindex 2>/dev/null &
fi
"#,
        vexp_binary, vexp_binary
    );
    write_hook(&post_merge, &post_merge_content)?;

    // Post-checkout hook: re-index on branch switch
    let post_checkout = hooks_dir.join("post-checkout");
    let post_checkout_content = format!(
        r#"#!/bin/sh
# Vexp: Re-index after branch switch
# Only run on branch checkout (not file checkout)
if [ "$3" = "1" ]; then
    if command -v {} >/dev/null 2>&1; then
        {} reindex 2>/dev/null &
    fi
fi
"#,
        vexp_binary, vexp_binary
    );
    write_hook(&post_checkout, &post_checkout_content)?;

    Ok(())
}

fn write_hook(path: &Path, content: &str) -> Result<()> {
    // Don't overwrite existing hooks that aren't ours
    if path.exists() {
        let existing = std::fs::read_to_string(path)?;
        if !existing.contains("Vexp:") {
            // Append to existing hook
            let appended = format!("{}\n\n{}", existing.trim_end(), content);
            std::fs::write(path, appended)?;
        } else {
            std::fs::write(path, content)?;
        }
    } else {
        std::fs::write(path, content)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}
