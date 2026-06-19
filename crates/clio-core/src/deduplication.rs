//! Memory deduplication detection and merging.
//!
//! Finds clusters of duplicate or near-duplicate memories using two strategies:
//! 1. Exact content matching via SQL GROUP BY
//! 2. FTS5 keyword similarity for near-duplicates
//!
//! Provides merge operations that combine tags, preserve the highest-confidence
//! version, reconcile links, and archive the merged-away memories.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::Memory;
use crate::repository;

/// A cluster of memories that are duplicates or near-duplicates.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateCluster {
    /// Memories in this cluster, ordered by updated_at descending.
    pub memories: Vec<Memory>,
    /// Similarity score (1.0 = exact match, lower = less similar).
    pub similarity: f64,
    /// How the match was detected: "exact" or "similar".
    pub match_type: String,
}

/// Result of a deduplication scan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateScanResult {
    pub clusters: Vec<DuplicateCluster>,
    pub total_scanned: u32,
    pub duplicates_found: u32,
}

/// Preview of what a merged memory would look like.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergePreview {
    pub keep_id: String,
    pub content: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub confidence: Option<f64>,
    pub importance: i32,
    pub namespace: String,
    pub kind: String,
    pub links_transferred: u32,
    pub memories_archived: u32,
}

/// Scan the database for duplicate and near-duplicate memories.
///
/// Performance target: completes within 10 seconds for 5,000 memories.
pub fn find_duplicates(conn: &Connection) -> Result<DuplicateScanResult> {
    let mut clusters = Vec::new();

    // Track IDs already assigned to a cluster to avoid overlap.
    let mut clustered_ids: HashSet<String> = HashSet::new();

    // 1. Find exact content duplicates (fast, SQL-only).
    let exact = find_exact_duplicates(conn)?;
    for cluster in &exact {
        for mem in &cluster.memories {
            clustered_ids.insert(mem.id.clone());
        }
    }
    clusters.extend(exact);

    // 2. Find near-duplicates via FTS5 keyword similarity.
    let near = find_near_duplicates(conn, &clustered_ids)?;
    for cluster in &near {
        for mem in &cluster.memories {
            clustered_ids.insert(mem.id.clone());
        }
    }
    clusters.extend(near);

    let duplicates_found: u32 = clusters
        .iter()
        .map(|c| c.memories.len() as u32)
        .sum();

    let total_scanned: u32 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE archived_at IS NULL",
        [],
        |row| row.get(0),
    )?;

    Ok(DuplicateScanResult {
        clusters,
        total_scanned,
        duplicates_found,
    })
}

/// Preview what a merged memory would look like without applying changes.
pub fn preview_merge(
    conn: &Connection,
    keep_id: &str,
    merge_ids: &[String],
) -> Result<MergePreview> {
    let keep = repository::get_raw(conn, keep_id)?;
    let mut all_tags: Vec<String> = keep.tags.clone();
    let mut best_confidence = keep.confidence;
    let mut best_importance = keep.importance;
    let mut links_transferred: u32 = 0;

    for merge_id in merge_ids {
        let mem = repository::get_raw(conn, merge_id)?;
        for tag in &mem.tags {
            if !all_tags.contains(tag) {
                all_tags.push(tag.clone());
            }
        }
        if let Some(conf) = mem.confidence {
            best_confidence = Some(match best_confidence {
                Some(c) => c.max(conf),
                None => conf,
            });
        }
        best_importance = best_importance.max(mem.importance);

        // Count links that would be transferred.
        let link_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM memory_links
             WHERE from_memory_id = ?1 OR to_memory_id = ?1",
            params![merge_id],
            |row| row.get(0),
        )?;
        links_transferred += link_count;
    }

    all_tags.sort();
    all_tags.dedup();

    Ok(MergePreview {
        keep_id: keep.id.clone(),
        content: keep.content.clone(),
        title: keep.title.clone(),
        tags: all_tags,
        confidence: best_confidence,
        importance: best_importance,
        namespace: keep.namespace.clone(),
        kind: keep.kind.clone(),
        links_transferred,
        memories_archived: merge_ids.len() as u32,
    })
}

/// Merge multiple memories into one.
///
/// - Tags from all memories are combined on the kept memory.
/// - Highest confidence and importance values are preserved.
/// - Incoming/outgoing links from merged memories are transferred to the kept memory.
/// - Merged-away memories are archived (not deleted).
pub fn merge_memories(
    conn: &Connection,
    keep_id: &str,
    merge_ids: &[String],
) -> Result<Memory> {
    let keep = repository::get_raw(conn, keep_id)?;
    let mut all_tags: Vec<String> = keep.tags.clone();
    let mut best_confidence = keep.confidence;
    let mut best_importance = keep.importance;

    // Collect data from memories being merged away.
    for merge_id in merge_ids {
        let mem = repository::get_raw(conn, merge_id)?;
        for tag in &mem.tags {
            if !all_tags.contains(tag) {
                all_tags.push(tag.clone());
            }
        }
        if let Some(conf) = mem.confidence {
            best_confidence = Some(match best_confidence {
                Some(c) => c.max(conf),
                None => conf,
            });
        }
        best_importance = best_importance.max(mem.importance);
    }

    all_tags.sort();
    all_tags.dedup();

    // Update the kept memory with merged metadata.
    let tags_text = all_tags.join(" ");
    let now = crate::models::now_utc();

    // Wrap the whole mutation in a savepoint so a mid-merge failure rolls back
    // cleanly instead of leaving the kept memory half-mutated.
    conn.execute_batch("SAVEPOINT merge_memories")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "UPDATE memories SET tags_text = ?1, confidence = ?2, importance = ?3, updated_at = ?4
             WHERE id = ?5",
            params![tags_text, best_confidence, best_importance, now, keep_id],
        )?;

        // Sync tags in the memory_tags table (created_at is NOT NULL).
        conn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![keep_id])?;
        for tag in &all_tags {
            conn.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag, created_at) VALUES (?1, ?2, ?3)",
                params![keep_id, tag, now],
            )?;
        }

        // Transfer links from merged memories to the kept memory.
        for merge_id in merge_ids {
            conn.execute(
                "UPDATE OR IGNORE memory_links SET from_memory_id = ?1
                 WHERE from_memory_id = ?2 AND to_memory_id != ?1",
                params![keep_id, merge_id],
            )?;
            conn.execute(
                "UPDATE OR IGNORE memory_links SET to_memory_id = ?1
                 WHERE to_memory_id = ?2 AND from_memory_id != ?1",
                params![keep_id, merge_id],
            )?;
            conn.execute(
                "DELETE FROM memory_links WHERE from_memory_id = ?1 OR to_memory_id = ?1",
                params![merge_id],
            )?;
            conn.execute(
                "DELETE FROM memory_links WHERE from_memory_id = ?1 AND to_memory_id = ?1",
                params![keep_id],
            )?;

            // Archive the merged-away memory.
            repository::archive(conn, merge_id)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("RELEASE merge_memories")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO merge_memories");
            let _ = conn.execute_batch("RELEASE merge_memories");
            return Err(e);
        }
    }

    // Return the updated kept memory without inflating its access_count.
    repository::get_raw(conn, keep_id)
}

// --- Internal helpers ---

/// Find groups of memories with identical content.
fn find_exact_duplicates(conn: &Connection) -> Result<Vec<DuplicateCluster>> {
    let mut stmt = conn.prepare(
        "SELECT GROUP_CONCAT(id, ',') AS ids
         FROM memories
         WHERE archived_at IS NULL
         GROUP BY content
         HAVING COUNT(*) > 1
         LIMIT 50",
    )?;

    let groups: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut clusters = Vec::new();
    for group in groups {
        let ids: Vec<String> = group.split(',').map(String::from).collect();
        let mut memories = Vec::new();
        for id in &ids {
            if let Ok(mem) = repository::get_raw(conn, id) {
                memories.push(mem);
            }
        }
        if memories.len() >= 2 {
            // Sort by updated_at descending so the most recently updated is first.
            memories.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            clusters.push(DuplicateCluster {
                memories,
                similarity: 1.0,
                match_type: "exact".into(),
            });
        }
    }

    Ok(clusters)
}

/// Find near-duplicate memories using FTS5 keyword similarity.
///
/// For each active memory, extract key terms and search FTS5 for similar content.
/// Pairs with high similarity are grouped into clusters.
fn find_near_duplicates(
    conn: &Connection,
    already_clustered: &HashSet<String>,
) -> Result<Vec<DuplicateCluster>> {
    // Load all active memory IDs with their titles and content excerpts.
    let mut stmt = conn.prepare(
        "SELECT id, title, content FROM memories
         WHERE archived_at IS NULL
         ORDER BY updated_at DESC",
    )?;
    let rows: Vec<(String, Option<String>, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Track similarity pairs: (id_a, id_b) -> similarity score.
    let mut pairs: HashMap<(String, String), f64> = HashMap::new();

    for (id, title, content) in &rows {
        if already_clustered.contains(id) {
            continue;
        }

        let query = build_fts_query(title.as_deref(), content);
        if query.is_empty() {
            continue;
        }

        // Query FTS5 for similar memories.
        let matches = conn.prepare_cached(
            "SELECT m.id, bm25(memory_fts) AS rank
             FROM memory_fts
             JOIN memories m ON m.rowid = memory_fts.rowid
             WHERE memory_fts MATCH ?1
               AND m.id != ?2
               AND m.archived_at IS NULL
             ORDER BY rank
             LIMIT 5",
        );

        let matches = match matches {
            Ok(mut search_stmt) => {
                let results: Vec<(String, f64)> = search_stmt
                    .query_map(params![query, id], |row| {
                        Ok((row.get(0)?, row.get::<_, f64>(1)?))
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                results
            }
            Err(_) => continue,
        };

        for (match_id, bm25_rank) in matches {
            if already_clustered.contains(&match_id) {
                continue;
            }
            // BM25 scores are negative (more negative = more relevant).
            // Convert to a 0-1 similarity score.
            let similarity = 1.0 / (1.0 + bm25_rank.abs());
            if similarity < 0.4 {
                continue;
            }

            // Also verify with word-level Jaccard similarity for accuracy.
            let content_a = content;
            let content_b = rows
                .iter()
                .find(|(mid, _, _)| mid == &match_id)
                .map(|(_, _, c)| c.as_str())
                .unwrap_or("");

            let jaccard = word_jaccard(content_a, content_b);
            if jaccard < 0.3 {
                continue;
            }

            // Use the higher of BM25-derived and Jaccard as the similarity.
            let final_sim = similarity.max(jaccard);

            let key = if id < &match_id {
                (id.clone(), match_id.clone())
            } else {
                (match_id.clone(), id.clone())
            };
            pairs.entry(key).or_insert(final_sim);
        }
    }

    // Group pairs into clusters using simple union-find.
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut cluster_sim: HashMap<String, f64> = HashMap::new();

    for ((a, b), sim) in &pairs {
        let root_a = find_root(&parent, a);
        let root_b = find_root(&parent, b);
        if root_a != root_b {
            parent.insert(root_b.clone(), root_a.clone());
            let existing = cluster_sim.get(&root_a).copied().unwrap_or(*sim);
            cluster_sim.insert(root_a, existing.min(*sim));
        }
    }

    // Collect cluster members.
    let mut cluster_members: HashMap<String, Vec<String>> = HashMap::new();
    let all_ids: HashSet<String> = pairs
        .keys()
        .flat_map(|(a, b)| vec![a.clone(), b.clone()])
        .collect();

    for id in &all_ids {
        let root = find_root(&parent, id);
        cluster_members
            .entry(root)
            .or_default()
            .push(id.clone());
    }

    // Build DuplicateCluster results.
    let mut clusters = Vec::new();
    for (root, member_ids) in cluster_members {
        if member_ids.len() < 2 {
            continue;
        }
        let sim = cluster_sim.get(&root).copied().unwrap_or(0.5);
        let mut memories = Vec::new();
        for id in &member_ids {
            if let Ok(mem) = repository::get_raw(conn, id) {
                memories.push(mem);
            }
        }
        if memories.len() >= 2 {
            memories.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            clusters.push(DuplicateCluster {
                memories,
                similarity: (sim * 100.0).round() / 100.0,
                match_type: "similar".into(),
            });
        }
    }

    // Sort clusters by similarity descending (most similar first).
    clusters.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Limit to 50 clusters.
    clusters.truncate(50);

    Ok(clusters)
}

/// Build an FTS5 query string from a memory's title and content.
///
/// Extracts significant words and joins them with OR for a broad match.
fn build_fts_query(title: Option<&str>, content: &str) -> String {
    let mut words: Vec<String> = Vec::new();

    // Add title words first (higher signal).
    if let Some(t) = title {
        for word in t.split_whitespace() {
            let clean = clean_word(word);
            if is_significant(&clean) {
                words.push(clean);
            }
        }
    }

    // Add content words (first ~200 chars to keep queries fast).
    let excerpt: String = content.chars().take(200).collect();
    for word in excerpt.split_whitespace() {
        let clean = clean_word(word);
        if is_significant(&clean) && !words.contains(&clean) {
            words.push(clean);
        }
        if words.len() >= 10 {
            break;
        }
    }

    if words.is_empty() {
        return String::new();
    }

    // Join with OR for broad matching. Wrap each in quotes to handle special chars.
    words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Compute word-level Jaccard similarity between two texts.
fn word_jaccard(a: &str, b: &str) -> f64 {
    let set_a: HashSet<String> = a
        .split_whitespace()
        .map(clean_word)
        .filter(|w| w.len() >= 3)
        .collect();
    let set_b: HashSet<String> = b
        .split_whitespace()
        .map(clean_word)
        .filter(|w| w.len() >= 3)
        .collect();

    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }

    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;

    intersection / union
}

/// Clean a word: lowercase, strip punctuation.
fn clean_word(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

/// Check if a word is significant enough to include in a query.
fn is_significant(word: &str) -> bool {
    if word.len() < 3 {
        return false;
    }
    // Skip common stop words.
    !matches!(
        word,
        "the" | "and" | "for" | "are" | "but" | "not" | "you" | "all"
            | "can" | "has" | "her" | "was" | "one" | "our" | "out"
            | "with" | "that" | "this" | "have" | "from" | "they"
            | "been" | "said" | "each" | "which" | "their" | "will"
            | "other" | "about" | "many" | "then" | "them" | "these"
            | "some" | "would" | "make" | "like" | "into" | "could"
            | "time" | "very" | "when" | "come" | "just" | "know"
            | "take" | "also" | "more" | "than" | "what" | "there"
    )
}

/// Union-find: find the root of a node.
fn find_root(parent: &HashMap<String, String>, id: &str) -> String {
    let mut current = id.to_string();
    while let Some(p) = parent.get(&current) {
        if p == &current {
            break;
        }
        current = p.clone();
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_word() {
        assert_eq!(clean_word("Hello!"), "hello");
        assert_eq!(clean_word("it's"), "its");
        assert_eq!(clean_word("snake_case"), "snake_case");
    }

    #[test]
    fn test_is_significant() {
        assert!(is_significant("database"));
        assert!(is_significant("memory"));
        assert!(!is_significant("the"));
        assert!(!is_significant("an"));
        assert!(!is_significant(""));
    }

    #[test]
    fn test_word_jaccard_identical() {
        let sim = word_jaccard("hello world foo", "hello world foo");
        assert!((sim - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_word_jaccard_different() {
        let sim = word_jaccard("alpha beta gamma", "delta epsilon zeta");
        assert!(sim < f64::EPSILON);
    }

    #[test]
    fn test_word_jaccard_partial() {
        let sim = word_jaccard("hello world foo bar", "hello world baz qux");
        assert!(sim > 0.0);
        assert!(sim < 1.0);
    }

    #[test]
    fn test_build_fts_query_with_title() {
        let q = build_fts_query(Some("Database migration plan"), "some content here");
        assert!(q.contains("\"database\""));
        assert!(q.contains("\"migration\""));
        assert!(q.contains("\"plan\""));
    }

    #[test]
    fn test_build_fts_query_empty_content() {
        let q = build_fts_query(None, "");
        assert!(q.is_empty());
    }

    #[test]
    fn test_find_root_simple() {
        let mut parent = HashMap::new();
        parent.insert("b".to_string(), "a".to_string());
        parent.insert("c".to_string(), "b".to_string());
        assert_eq!(find_root(&parent, "c"), "a");
    }
}
