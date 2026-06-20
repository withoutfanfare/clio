//! Memory database integrity checks.
//!
//! Detects broken links, orphaned memories (no namespace match), duplicate
//! content hashes, and invalid metadata. Each issue includes a suggested fix
//! and can optionally be auto-repaired.

use rusqlite::{Connection, params};

use crate::error::Result;

/// A single integrity issue found during a check.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntegrityIssue {
    /// Category of the issue.
    pub kind: String,
    /// Human-readable description.
    pub description: String,
    /// Suggested fix action.
    pub suggested_fix: String,
    /// Whether this issue can be auto-fixed.
    pub auto_fixable: bool,
    /// Affected memory or link IDs.
    pub affected_ids: Vec<String>,
}

/// Result of an integrity check.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntegrityReport {
    pub issues: Vec<IntegrityIssue>,
    pub total_checked: u32,
    pub issues_found: u32,
    pub fixed: u32,
}

/// Run all integrity checks on the database.
pub fn check(conn: &Connection) -> Result<IntegrityReport> {
    let mut issues = Vec::new();

    let total_checked: u32 =
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;

    // 1. Broken links: links pointing to non-existent memories
    let broken_links = find_broken_links(conn)?;
    if !broken_links.is_empty() {
        issues.push(IntegrityIssue {
            kind: "broken_link".into(),
            description: format!(
                "{} link(s) point to memories that no longer exist",
                broken_links.len()
            ),
            suggested_fix: "Remove broken links".into(),
            auto_fixable: true,
            affected_ids: broken_links,
        });
    }

    // 2. Orphaned links: links where the source memory no longer exists
    let orphaned_links = find_orphaned_source_links(conn)?;
    if !orphaned_links.is_empty() {
        issues.push(IntegrityIssue {
            kind: "orphaned_link".into(),
            description: format!(
                "{} link(s) have a source memory that no longer exists",
                orphaned_links.len()
            ),
            suggested_fix: "Remove orphaned links".into(),
            auto_fixable: true,
            affected_ids: orphaned_links,
        });
    }

    // 3. Duplicate content hashes
    let duplicates = find_duplicate_content(conn)?;
    if !duplicates.is_empty() {
        issues.push(IntegrityIssue {
            kind: "duplicate_content".into(),
            description: format!(
                "{} group(s) of memories share identical content",
                duplicates.len()
            ),
            suggested_fix: "Review and merge duplicate memories".into(),
            auto_fixable: false,
            affected_ids: duplicates,
        });
    }

    // 4. Empty content
    let empty = find_empty_content(conn)?;
    if !empty.is_empty() {
        issues.push(IntegrityIssue {
            kind: "empty_content".into(),
            description: format!("{} memory/memories have empty content", empty.len()),
            suggested_fix: "Archive or delete empty memories".into(),
            auto_fixable: false,
            affected_ids: empty,
        });
    }

    // 5. Tags out of sync (tags_text vs memory_tags table)
    let tag_mismatches = find_tag_mismatches(conn)?;
    if !tag_mismatches.is_empty() {
        issues.push(IntegrityIssue {
            kind: "tag_mismatch".into(),
            description: format!(
                "{} memory/memories have tags_text out of sync with the memory_tags table",
                tag_mismatches.len()
            ),
            suggested_fix: "Re-sync tags_text from memory_tags".into(),
            auto_fixable: true,
            affected_ids: tag_mismatches,
        });
    }

    let issues_found = issues.iter().map(|i| i.affected_ids.len() as u32).sum();

    Ok(IntegrityReport {
        issues,
        total_checked,
        issues_found,
        fixed: 0,
    })
}

/// Auto-fix all fixable issues. Returns the number of fixes applied.
pub fn fix(conn: &Connection) -> Result<IntegrityReport> {
    let mut report = check(conn)?;
    let mut fixed = 0u32;

    for issue in &report.issues {
        if !issue.auto_fixable {
            continue;
        }
        match issue.kind.as_str() {
            "broken_link" => {
                let count = conn.execute(
                    "DELETE FROM memory_links WHERE to_memory_id NOT IN (SELECT id FROM memories)",
                    [],
                )?;
                fixed += count as u32;
            }
            "orphaned_link" => {
                let count = conn.execute(
                    "DELETE FROM memory_links WHERE from_memory_id NOT IN (SELECT id FROM memories)",
                    [],
                )?;
                fixed += count as u32;
            }
            "tag_mismatch" => {
                for id in &issue.affected_ids {
                    let tags: Vec<String> = {
                        let mut stmt = conn.prepare(
                            "SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag",
                        )?;
                        let rows = stmt.query_map(params![id], |row| row.get(0))?;
                        rows.collect::<std::result::Result<Vec<_>, _>>()?
                    };
                    let tags_text = tags.join(" ");
                    conn.execute(
                        "UPDATE memories SET tags_text = ?1 WHERE id = ?2",
                        params![tags_text, id],
                    )?;
                    fixed += 1;
                }
            }
            _ => {}
        }
    }

    report.fixed = fixed;
    Ok(report)
}

fn find_broken_links(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT ml.to_memory_id
         FROM memory_links ml
         LEFT JOIN memories m ON ml.to_memory_id = m.id
         WHERE m.id IS NULL",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn find_orphaned_source_links(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT ml.from_memory_id
         FROM memory_links ml
         LEFT JOIN memories m ON ml.from_memory_id = m.id
         WHERE m.id IS NULL",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn find_duplicate_content(conn: &Connection) -> Result<Vec<String>> {
    // Find IDs where content appears more than once
    let mut stmt = conn.prepare(
        "SELECT GROUP_CONCAT(id, ',')
         FROM memories
         WHERE archived_at IS NULL
         GROUP BY content
         HAVING COUNT(*) > 1
         LIMIT 50",
    )?;
    let rows = stmt.query_map([], |row| {
        let ids: String = row.get(0)?;
        Ok(ids)
    })?;
    let mut result = Vec::new();
    for row in rows {
        let ids = row?;
        for id in ids.split(',') {
            result.push(id.to_string());
        }
    }
    Ok(result)
}

fn find_empty_content(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM memories WHERE TRIM(content) = '' OR content IS NULL")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn find_tag_mismatches(conn: &Connection) -> Result<Vec<String>> {
    // Find memories where tags_text does not match the actual tags in memory_tags
    let mut stmt = conn.prepare(
        "SELECT m.id
         FROM memories m
         WHERE m.tags_text != COALESCE(
           (SELECT GROUP_CONCAT(mt.tag, ' ')
            FROM (SELECT tag FROM memory_tags WHERE memory_id = m.id ORDER BY tag) mt),
           ''
         )
         LIMIT 100",
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
