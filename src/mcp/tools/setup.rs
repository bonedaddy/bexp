use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, SetupParams};

pub async fn handle(server: &BexpServer, params: SetupParams) -> Result<CallToolResult, ErrorData> {
    let bexp_dir = server.workspace_root.join(".bexp");
    let config_path = bexp_dir.join("config.toml");
    let force = params.force.unwrap_or(false);

    if config_path.exists() && !force {
        return Ok(CallToolResult::success(vec![Content::text(
            "`.bexp/config.toml` already exists. Use `force: true` to regenerate.",
        )]));
    }

    // Detect project type
    let project_type = detect_project_type(&server.workspace_root);

    let config_content = generate_config(&project_type);

    std::fs::create_dir_all(&bexp_dir).map_err(super::to_error_data)?;

    std::fs::write(&config_path, &config_content).map_err(super::to_error_data)?;

    // Create .gitignore for .bexp
    let gitignore_path = bexp_dir.join(".gitignore");
    if !gitignore_path.exists() || force {
        std::fs::write(&gitignore_path, "index.db\nindex.db-wal\nindex.db-shm\n")
            .map_err(super::to_error_data)?;
    }

    let mut output = String::new();
    output.push_str("# Workspace Setup Complete\n\n");
    output.push_str(&format!(
        "**Detected project type:** {}\n\n",
        project_type.name
    ));
    output.push_str("**Created files:**\n");
    output.push_str("- `.bexp/config.toml`\n");
    output.push_str("- `.bexp/.gitignore`\n\n");
    output.push_str("**Configuration:**\n");
    output.push_str(&format!("```toml\n{}\n```\n", config_content));

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[allow(dead_code)]
struct ProjectType {
    name: String,
    languages: Vec<String>,
    extra_excludes: Vec<String>,
}

fn detect_project_type(root: &std::path::Path) -> ProjectType {
    let mut languages = Vec::new();
    let mut extra_excludes = Vec::new();
    let mut name = "generic".to_string();

    if root.join("package.json").exists() {
        languages.push("typescript".into());
        languages.push("javascript".into());
        extra_excludes.push("node_modules".into());
        name = if root.join("tsconfig.json").exists() {
            "typescript-node".into()
        } else {
            "javascript-node".into()
        };
        if root.join("next.config.js").exists()
            || root.join("next.config.mjs").exists()
            || root.join("next.config.ts").exists()
        {
            name = "nextjs".into();
            extra_excludes.push(".next".into());
        }
    }

    if root.join("Cargo.toml").exists() {
        languages.push("rust".into());
        extra_excludes.push("target".into());
        if name == "generic" {
            name = "rust".into();
        }
    }

    if root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
        || root.join("requirements.txt").exists()
    {
        languages.push("python".into());
        extra_excludes.push("__pycache__".into());
        extra_excludes.push(".venv".into());
        extra_excludes.push("venv".into());
        if name == "generic" {
            name = "python".into();
        }
    }

    if languages.is_empty() {
        languages.extend(
            ["typescript", "javascript", "python", "rust"]
                .iter()
                .map(|s| s.to_string()),
        );
    }

    ProjectType {
        name,
        languages,
        extra_excludes,
    }
}

fn generate_config(project_type: &ProjectType) -> String {
    let mut config = String::new();
    config.push_str("# bexp Configuration\n");
    config.push_str(&format!(
        "# Detected project type: {}\n\n",
        project_type.name
    ));
    config.push_str("token_budget = 8000\n");
    config.push_str("default_skeleton_level = \"standard\"\n");
    config.push_str("max_file_size = 1000000\n");
    config.push_str("watcher_debounce_ms = 500\n");
    config.push_str("memory_budget_pct = 0.10\n");
    config.push_str("session_compress_after_hours = 2\n");
    config.push_str("observation_ttl_days = 90\n");
    config.push_str("db_path = \".bexp/index.db\"\n\n");

    let mut excludes: Vec<String> = vec![
        "node_modules",
        ".git",
        "target",
        "dist",
        "build",
        "__pycache__",
        ".venv",
        "venv",
        ".next",
        ".nuxt",
        "vendor",
        ".bexp",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    for e in &project_type.extra_excludes {
        if !excludes.contains(e) {
            excludes.push(e.clone());
        }
    }

    config.push_str("exclude_patterns = [\n");
    for (i, e) in excludes.iter().enumerate() {
        config.push_str(&format!("    \"{}\"", e));
        if i < excludes.len() - 1 {
            config.push(',');
        }
        config.push('\n');
    }
    config.push_str("]\n");

    config
}
