pub mod hooks;

use std::path::{Path, PathBuf};

/// Discover git submodule paths within the workspace.
/// Parses `.gitmodules` directly (no subprocess dependency on `git`).
/// Returns absolute paths to each submodule directory that exists on disk.
/// Returns an empty vec if `.gitmodules` doesn't exist or can't be parsed.
pub fn discover_submodules(workspace_root: &Path) -> Vec<PathBuf> {
    let gitmodules_path = workspace_root.join(".gitmodules");
    let content = match std::fs::read_to_string(&gitmodules_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut paths = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("path") {
            let value = value.trim_start();
            if let Some(value) = value.strip_prefix('=') {
                let rel_path = value.trim();
                if !rel_path.is_empty() {
                    let abs_path = workspace_root.join(rel_path);
                    if abs_path.is_dir() {
                        paths.push(abs_path);
                    }
                }
            }
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_submodules_no_gitmodules() {
        let tmp = tempfile::tempdir().unwrap();
        let result = discover_submodules(tmp.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_discover_submodules_parses_paths() {
        let tmp = tempfile::tempdir().unwrap();

        // Create submodule directories
        fs::create_dir_all(tmp.path().join("lib/foo")).unwrap();
        fs::create_dir_all(tmp.path().join("vendor/bar")).unwrap();

        // Write .gitmodules
        fs::write(
            tmp.path().join(".gitmodules"),
            r#"[submodule "foo"]
	path = lib/foo
	url = https://github.com/example/foo.git
[submodule "bar"]
	path = vendor/bar
	url = https://github.com/example/bar.git
[submodule "missing"]
	path = does/not/exist
	url = https://github.com/example/missing.git
"#,
        )
        .unwrap();

        let result = discover_submodules(tmp.path());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], tmp.path().join("lib/foo"));
        assert_eq!(result[1], tmp.path().join("vendor/bar"));
    }
}
