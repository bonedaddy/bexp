pub mod hooks;

use std::path::Path;


pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}
