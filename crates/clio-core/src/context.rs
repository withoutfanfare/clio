//! Automatic namespace detection from the working directory.
//!
//! Walks up from a given path looking for project markers (`.clio-namespace`
//! file, `.git` directory, `Cargo.toml`, `package.json`, `CLAUDE.md`) and
//! derives a namespace string from the first match.

use std::path::Path;

/// Information about the detected project context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DetectedContext {
    /// The resolved namespace, e.g. `project:clio`.
    pub namespace: String,

    /// How the namespace was detected.
    pub source: DetectionSource,

    /// The directory where the marker was found.
    pub marker_path: String,
}

/// How a namespace was detected.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    /// An explicit `.clio-namespace` file.
    ClioNamespaceFile,
    /// A `.git` directory (repo name used as slug).
    GitDirectory,
    /// A project manifest file (`Cargo.toml`, `package.json`).
    ProjectManifest,
}

impl std::fmt::Display for DetectionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClioNamespaceFile => write!(f, ".clio-namespace file"),
            Self::GitDirectory => write!(f, ".git directory"),
            Self::ProjectManifest => write!(f, "project manifest"),
        }
    }
}

/// Walk up from `cwd` looking for project markers and return the detected
/// namespace, or `None` if no project context is found.
pub fn detect_namespace(cwd: &Path) -> Option<DetectedContext> {
    let mut dir = cwd.to_path_buf();

    loop {
        // Priority 1: explicit .clio-namespace file
        let ns_file = dir.join(".clio-namespace");
        if ns_file.is_file() {
            if let Ok(content) = std::fs::read_to_string(&ns_file) {
                let namespace = content.trim().to_string();
                if !namespace.is_empty()
                    && namespace.len() <= 120
                    && !namespace.chars().any(|c| c.is_control())
                {
                    return Some(DetectedContext {
                        namespace,
                        source: DetectionSource::ClioNamespaceFile,
                        marker_path: dir.display().to_string(),
                    });
                }
            }
        }

        // Priority 2: .git directory — derive project:<repo-name>
        let git_dir = dir.join(".git");
        if git_dir.exists() {
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                let slug = slugify(name);
                if !slug.is_empty() {
                    return Some(DetectedContext {
                        namespace: format!("project:{slug}"),
                        source: DetectionSource::GitDirectory,
                        marker_path: dir.display().to_string(),
                    });
                }
            }
        }

        // Priority 3: project manifest files — derive project:<dir-name>
        let manifests = ["Cargo.toml", "package.json"];
        for manifest in manifests {
            if dir.join(manifest).is_file() {
                if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                    let slug = slugify(name);
                    if !slug.is_empty() {
                        return Some(DetectedContext {
                            namespace: format!("project:{slug}"),
                            source: DetectionSource::ProjectManifest,
                            marker_path: dir.display().to_string(),
                        });
                    }
                }
            }
        }

        // Move up one level.
        if !dir.pop() {
            break;
        }
    }

    None
}

/// Resolve the effective namespace for a command invocation.
///
/// - If `explicit` is `Some`, it takes priority (user passed `--namespace`).
/// - If `auto_detect` is true and no explicit namespace was given, attempt
///   detection from `cwd`.
/// - Falls back to `"global"`.
pub fn resolve_namespace(explicit: Option<&str>, cwd: Option<&Path>, auto_detect: bool) -> String {
    if let Some(ns) = explicit {
        return ns.to_string();
    }

    if auto_detect {
        if let Some(cwd) = cwd {
            if let Some(ctx) = detect_namespace(cwd) {
                return ctx.namespace;
            }
        }
    }

    "global".to_string()
}

/// Resolve the effective namespace for a command invocation, returning the full
/// detection context when available.
pub fn resolve_namespace_with_context(
    explicit: Option<&str>,
    cwd: Option<&Path>,
    auto_detect: bool,
) -> (String, Option<DetectedContext>) {
    if let Some(ns) = explicit {
        return (ns.to_string(), None);
    }

    if auto_detect {
        if let Some(cwd) = cwd {
            if let Some(ctx) = detect_namespace(cwd) {
                let ns = ctx.namespace.clone();
                return (ns, Some(ctx));
            }
        }
    }

    ("global".to_string(), None)
}

/// Create a `.clio-namespace` file in the given directory.
///
/// Validates that the namespace is 1–120 characters and contains no control
/// characters (mirroring the rules in [`detect_namespace`]).
pub fn init_namespace(dir: &Path, namespace: &str) -> std::io::Result<()> {
    if namespace.is_empty() || namespace.len() > 120 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "namespace must be between 1 and 120 characters",
        ));
    }
    if namespace.chars().any(|c| c.is_control()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "namespace must not contain control characters",
        ));
    }
    let path = dir.join(".clio-namespace");
    std::fs::write(&path, format!("{namespace}\n"))
}

/// Convert a directory name into a URL-safe slug.
pub fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Project"), "my-project");
        assert_eq!(slugify("clio-core"), "clio-core");
        assert_eq!(slugify("UPPERCASE"), "uppercase");
        assert_eq!(slugify("with.dots"), "with-dots");
    }

    #[test]
    fn detect_clio_namespace_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join(".clio-namespace"), "project:my-app\n").unwrap();

        let ctx = detect_namespace(root).expect("should detect namespace");
        assert_eq!(ctx.namespace, "project:my-app");
        assert!(matches!(ctx.source, DetectionSource::ClioNamespaceFile));
    }

    #[test]
    fn detect_git_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();

        let ctx = detect_namespace(&root).expect("should detect namespace");
        assert_eq!(ctx.namespace, "project:my-repo");
        assert!(matches!(ctx.source, DetectionSource::GitDirectory));
    }

    #[test]
    fn detect_cargo_manifest() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("rust-project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let ctx = detect_namespace(&root).expect("should detect namespace");
        assert_eq!(ctx.namespace, "project:rust-project");
        assert!(matches!(ctx.source, DetectionSource::ProjectManifest));
    }

    #[test]
    fn detect_package_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("js-project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();

        let ctx = detect_namespace(&root).expect("should detect namespace");
        assert_eq!(ctx.namespace, "project:js-project");
        assert!(matches!(ctx.source, DetectionSource::ProjectManifest));
    }

    #[test]
    fn clio_namespace_file_takes_priority_over_git() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".clio-namespace"), "tool:custom\n").unwrap();

        let ctx = detect_namespace(&root).expect("should detect namespace");
        assert_eq!(ctx.namespace, "tool:custom");
        assert!(matches!(ctx.source, DetectionSource::ClioNamespaceFile));
    }

    #[test]
    fn walks_up_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let sub = root.join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();

        let ctx = detect_namespace(&sub).expect("should detect namespace from ancestor");
        assert_eq!(ctx.namespace, "project:my-repo");
    }

    #[test]
    fn returns_none_when_no_markers() {
        let tmp = TempDir::new().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        // Detect from an isolated empty directory (not walking into system dirs).
        assert!(detect_namespace(&empty).is_none());
    }

    #[test]
    fn resolve_explicit_overrides_detection() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();

        let ns = resolve_namespace(Some("custom:override"), Some(&root), true);
        assert_eq!(ns, "custom:override");
    }

    #[test]
    fn resolve_falls_back_to_global() {
        let tmp = TempDir::new().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let ns = resolve_namespace(None, Some(&empty), true);
        assert_eq!(ns, "global");
    }

    #[test]
    fn resolve_disabled_auto_detect() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("my-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();

        let ns = resolve_namespace(None, Some(&root), false);
        assert_eq!(ns, "global");
    }

    #[test]
    fn init_creates_namespace_file() {
        let tmp = TempDir::new().unwrap();
        init_namespace(tmp.path(), "project:test").unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".clio-namespace")).unwrap();
        assert_eq!(content.trim(), "project:test");
    }

    #[test]
    fn init_rejects_empty_namespace() {
        let tmp = TempDir::new().unwrap();
        assert!(init_namespace(tmp.path(), "").is_err());
    }

    #[test]
    fn init_rejects_control_chars() {
        let tmp = TempDir::new().unwrap();
        assert!(init_namespace(tmp.path(), "bad\x00ns").is_err());
    }

    #[test]
    fn init_rejects_overlong_namespace() {
        let tmp = TempDir::new().unwrap();
        let long = "a".repeat(121);
        assert!(init_namespace(tmp.path(), &long).is_err());
    }
}
