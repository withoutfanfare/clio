//! Namespace cleanup: detect stale namespaces and purge them (with a backup).
//!
//! Detection is the "automatic" part — `find_candidates` flags namespaces that
//! match any of the configured criteria (stale by age, all memories archived,
//! or the project folder no longer on disk). Purging is explicit and always
//! takes a database backup first via [`execute_cleanup`].

use crate::backup;
use crate::context::slugify;
use crate::error::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// Why a namespace was flagged as a cleanup candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupReason {
    /// No activity within the configured staleness window.
    StaleByAge,
    /// Every memory in the namespace is archived (no live memories).
    AllArchived,
    /// A `project:<slug>` namespace whose folder was not found under any dev root.
    FolderGone,
}

/// A namespace flagged for potential cleanup, with the reasons it was flagged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupCandidate {
    pub namespace: String,
    pub live_count: i64,
    pub archived_count: i64,
    pub last_activity: Option<String>,
    pub reasons: Vec<CleanupReason>,
}

/// Which checks to apply when finding candidates.
#[derive(Debug, Clone)]
pub struct CleanupCriteria {
    /// Flag namespaces with no activity for this many months. `None` skips the check.
    pub stale_months: Option<u32>,
    /// Flag namespaces whose memories are all archived.
    pub all_archived: bool,
    /// Flag `project:<slug>` namespaces whose folder is missing from disk.
    pub folder_gone: bool,
}

/// Outcome of an executed cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub backup_path: Option<String>,
    pub namespaces_deleted: Vec<String>,
    pub memories_purged: usize,
}

struct NamespaceRow {
    namespace: String,
    live: i64,
    archived: i64,
    last_activity: Option<String>,
}

fn load_namespace_rows(conn: &Connection) -> Result<Vec<NamespaceRow>> {
    let mut stmt = conn.prepare(
        "SELECT namespace,
                SUM(CASE WHEN archived_at IS NULL THEN 1 ELSE 0 END) AS live,
                SUM(CASE WHEN archived_at IS NOT NULL THEN 1 ELSE 0 END) AS archived,
                MAX(COALESCE(archived_at, updated_at)) AS last_activity
         FROM memories
         GROUP BY namespace
         ORDER BY namespace",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(NamespaceRow {
            namespace: row.get(0)?,
            live: row.get(1)?,
            archived: row.get(2)?,
            last_activity: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Expand a list of configured dev-root strings into absolute paths, replacing
/// a leading `~` with `$HOME`.
pub fn expand_dev_roots(roots: &[String]) -> Vec<PathBuf> {
    expand_dev_roots_with_home(roots, std::env::var("HOME").ok().as_deref())
}

fn expand_dev_roots_with_home(roots: &[String], home: Option<&str>) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|r| match (r.strip_prefix("~/"), home) {
            (Some(rest), Some(h)) => PathBuf::from(h).join(rest),
            _ => PathBuf::from(r),
        })
        .collect()
}

/// Find namespaces that match the cleanup criteria. `now` is injected for
/// testability. `dev_roots` must already be expanded to absolute paths; they
/// are scanned for the "folder gone" heuristic (which only applies to
/// `project:<slug>` namespaces). The `global` namespace is never flagged.
pub fn find_candidates(
    conn: &Connection,
    criteria: &CleanupCriteria,
    dev_roots: &[PathBuf],
    now: OffsetDateTime,
) -> Result<Vec<CleanupCandidate>> {
    let rows = load_namespace_rows(conn)?;

    // For the folder-gone check, gather every project slug present on disk once.
    let disk_slugs = if criteria.folder_gone {
        Some(collect_disk_slugs(dev_roots))
    } else {
        None
    };

    let mut candidates = Vec::new();
    for row in rows {
        if row.namespace == "global" || row.namespace.is_empty() {
            continue;
        }

        let mut reasons = Vec::new();

        if criteria.all_archived && row.live == 0 {
            reasons.push(CleanupReason::AllArchived);
        }

        if let Some(months) = criteria.stale_months {
            if let Some(last) = &row.last_activity {
                if is_older_than_months(last, months, now) {
                    reasons.push(CleanupReason::StaleByAge);
                }
            }
        }

        if criteria.folder_gone {
            if let Some(slug) = project_slug(&row.namespace) {
                let gone = match recorded_cwd(conn, &row.namespace) {
                    // A recorded working directory gives a reliable answer.
                    Some(cwd) => !Path::new(&cwd).exists(),
                    // Otherwise fall back to the slug-on-disk heuristic.
                    None => disk_slugs
                        .as_ref()
                        .map(|slugs| !slugs.contains(&slug))
                        .unwrap_or(false),
                };
                if gone {
                    reasons.push(CleanupReason::FolderGone);
                }
            }
        }

        if !reasons.is_empty() {
            candidates.push(CleanupCandidate {
                namespace: row.namespace,
                live_count: row.live,
                archived_count: row.archived,
                last_activity: row.last_activity,
                reasons,
            });
        }
    }

    Ok(candidates)
}

/// Convenience wrapper over [`find_candidates`] using the current time, so
/// adapters need not depend on the `time` crate.
pub fn find_candidates_now(
    conn: &Connection,
    criteria: &CleanupCriteria,
    dev_roots: &[PathBuf],
) -> Result<Vec<CleanupCandidate>> {
    find_candidates(conn, criteria, dev_roots, OffsetDateTime::now_utc())
}

fn project_slug(namespace: &str) -> Option<String> {
    namespace.strip_prefix("project:").map(str::to_string)
}

/// The most recently recorded working directory for a namespace, read from the
/// `cwd` key of a memory's metadata. Returns `None` if no memory recorded one.
fn recorded_cwd(conn: &Connection, namespace: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT metadata_json FROM memories
             WHERE namespace = ?1 AND metadata_json != '{}'
             ORDER BY updated_at DESC LIMIT 25",
        )
        .ok()?;
    let rows = stmt
        .query_map([namespace], |row| row.get::<_, String>(0))
        .ok()?;
    for meta in rows.flatten() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&meta) {
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                if !cwd.is_empty() {
                    return Some(cwd.to_string());
                }
            }
        }
    }
    None
}

fn is_older_than_months(last_activity: &str, months: u32, now: OffsetDateTime) -> bool {
    match OffsetDateTime::parse(
        last_activity,
        &time::format_description::well_known::Rfc3339,
    ) {
        // 30-day month approximation — precise enough for a months-scale window.
        Ok(ts) => ts < now - time::Duration::days(months as i64 * 30),
        Err(_) => false,
    }
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
];

/// Markers that identify a directory as a project root — the same signals
/// `context::detect_namespace` uses to derive `project:<slug>`.
const PROJECT_MARKERS: &[&str] = &[".git", "Cargo.toml", "package.json", ".clio-namespace"];

/// Walk the dev roots and collect the slug of every *project root* directory
/// (those bearing a `.git`/manifest marker). Recursion prunes at a project
/// root — projects don't nest into other namespaces — which keeps the scan fast
/// even over large trees. A namespace whose slug is absent is "folder gone".
fn collect_disk_slugs(dev_roots: &[PathBuf]) -> HashSet<String> {
    let mut slugs = HashSet::new();
    for root in dev_roots {
        scan_dir(root, 0, 8, &mut slugs);
    }
    slugs
}

fn is_project_root(dir: &Path) -> bool {
    PROJECT_MARKERS.iter().any(|m| dir.join(m).exists())
}

fn scan_dir(dir: &Path, depth: u32, max_depth: u32, slugs: &mut HashSet<String>) {
    if depth > max_depth {
        return;
    }

    // A project root yields its slug and is not descended into — namespaces
    // come from project roots, not their sub-directories.
    if depth > 0 && is_project_root(dir) {
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            slugs.insert(slugify(name));
        }
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') || SKIP_DIRS.contains(&name) {
            continue;
        }
        scan_dir(&path, depth + 1, max_depth, slugs);
    }
}

/// Purge the given namespaces and all their memories, taking a database backup
/// first. The `global` namespace cannot be purged (rejected by the repository).
pub fn execute_cleanup(
    conn: &Connection,
    db_path: &Path,
    namespaces: &[String],
    max_backups: u32,
) -> Result<CleanupReport> {
    if namespaces.is_empty() {
        return Ok(CleanupReport {
            backup_path: None,
            namespaces_deleted: Vec::new(),
            memories_purged: 0,
        });
    }

    // Always back up before a destructive purge.
    let backup_result = backup::backup(db_path, None, max_backups)?;

    let mut purged = 0usize;
    let mut deleted = Vec::new();
    for ns in namespaces {
        purged += crate::repository::delete_namespace_with_memories(conn, ns)?;
        deleted.push(ns.clone());
    }

    Ok(CleanupReport {
        backup_path: Some(backup_result.path),
        namespaces_deleted: deleted,
        memories_purged: purged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RememberInput;
    use crate::settings::Settings;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn insert(conn: &Connection, namespace: &str, updated_at: &str, archived: bool) {
        let s = Settings::default();
        let input = RememberInput {
            namespace: namespace.to_string(),
            kind: "note".into(),
            title: Some("t".into()),
            summary: None,
            content: "c".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        };
        let m = crate::repository::remember(conn, &input, &s).unwrap();
        // Force timestamps / archive state directly for deterministic tests.
        if archived {
            conn.execute(
                "UPDATE memories SET updated_at = ?1, archived_at = ?1 WHERE id = ?2",
                rusqlite::params![updated_at, m.id],
            )
            .unwrap();
        } else {
            conn.execute(
                "UPDATE memories SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![updated_at, m.id],
            )
            .unwrap();
        }
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse("2026-06-20T00:00:00Z", &time::format_description::well_known::Rfc3339)
            .unwrap()
    }

    #[test]
    fn flags_stale_by_age_but_not_recent() {
        let conn = test_conn();
        insert(&conn, "project:old", "2025-01-01T00:00:00Z", false); // ~17 months ago
        insert(&conn, "project:fresh", "2026-06-10T00:00:00Z", false); // 10 days ago

        let criteria = CleanupCriteria {
            stale_months: Some(6),
            all_archived: false,
            folder_gone: false,
        };
        let found = find_candidates(&conn, &criteria, &[], now()).unwrap();
        let names: Vec<_> = found.iter().map(|c| c.namespace.as_str()).collect();
        assert_eq!(names, vec!["project:old"]);
        assert_eq!(found[0].reasons, vec![CleanupReason::StaleByAge]);
    }

    #[test]
    fn flags_all_archived() {
        let conn = test_conn();
        insert(&conn, "project:dead", "2026-06-19T00:00:00Z", true);
        insert(&conn, "project:live", "2026-06-19T00:00:00Z", false);

        let criteria = CleanupCriteria {
            stale_months: None,
            all_archived: true,
            folder_gone: false,
        };
        let found = find_candidates(&conn, &criteria, &[], now()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].namespace, "project:dead");
        assert_eq!(found[0].live_count, 0);
        assert_eq!(found[0].reasons, vec![CleanupReason::AllArchived]);
    }

    #[test]
    fn never_flags_global() {
        let conn = test_conn();
        insert(&conn, "global", "2020-01-01T00:00:00Z", true);
        let criteria = CleanupCriteria {
            stale_months: Some(6),
            all_archived: true,
            folder_gone: false,
        };
        let found = find_candidates(&conn, &criteria, &[], now()).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn folder_gone_flags_missing_only() {
        let conn = test_conn();
        let tmp = tempfile::tempdir().unwrap();
        // A real project root: a directory bearing a project marker.
        std::fs::create_dir(tmp.path().join("present")).unwrap();
        std::fs::write(tmp.path().join("present").join("Cargo.toml"), "").unwrap();
        insert(&conn, "project:present", "2026-06-19T00:00:00Z", false);
        insert(&conn, "project:missing", "2026-06-19T00:00:00Z", false);

        let criteria = CleanupCriteria {
            stale_months: None,
            all_archived: false,
            folder_gone: true,
        };
        let found = find_candidates(&conn, &criteria, &[tmp.path().to_path_buf()], now()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].namespace, "project:missing");
        assert_eq!(found[0].reasons, vec![CleanupReason::FolderGone]);
    }

    fn insert_with_cwd(conn: &Connection, namespace: &str, cwd: &str) {
        let s = Settings::default();
        let input = RememberInput {
            namespace: namespace.to_string(),
            kind: "note".into(),
            title: Some("t".into()),
            summary: None,
            content: "c".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({ "cwd": cwd }),
            valid_from: None,
            valid_until: None,
            upsert: false,
        };
        crate::repository::remember(conn, &input, &s).unwrap();
    }

    #[test]
    fn folder_gone_prefers_recorded_cwd() {
        let conn = test_conn();
        let tmp = tempfile::tempdir().unwrap();
        let present = tmp.path().join("here");
        std::fs::create_dir(&present).unwrap();

        // Recorded cwd exists → not flagged, even with no dev-root match.
        insert_with_cwd(&conn, "project:here", present.to_str().unwrap());
        // Recorded cwd is gone → flagged reliably.
        insert_with_cwd(
            &conn,
            "project:vanished",
            tmp.path().join("vanished").to_str().unwrap(),
        );

        let criteria = CleanupCriteria {
            stale_months: None,
            all_archived: false,
            folder_gone: true,
        };
        // No dev roots: heuristic would flag both; recorded cwd must override.
        let found = find_candidates(&conn, &criteria, &[], now()).unwrap();
        let names: Vec<_> = found.iter().map(|c| c.namespace.as_str()).collect();
        assert_eq!(names, vec!["project:vanished"]);
    }

    #[test]
    fn combines_multiple_reasons() {
        let conn = test_conn();
        let tmp = tempfile::tempdir().unwrap();
        insert(&conn, "project:gone", "2024-01-01T00:00:00Z", true);
        let criteria = CleanupCriteria {
            stale_months: Some(6),
            all_archived: true,
            folder_gone: true,
        };
        let found = find_candidates(&conn, &criteria, &[tmp.path().to_path_buf()], now()).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].reasons.contains(&CleanupReason::StaleByAge));
        assert!(found[0].reasons.contains(&CleanupReason::AllArchived));
        assert!(found[0].reasons.contains(&CleanupReason::FolderGone));
    }

    #[test]
    fn expand_dev_roots_replaces_tilde() {
        let roots = expand_dev_roots_with_home(
            &["~/Development".into(), "/abs/path".into()],
            Some("/home/test"),
        );
        assert_eq!(roots[0], PathBuf::from("/home/test/Development"));
        assert_eq!(roots[1], PathBuf::from("/abs/path"));
    }
}
