use std::io::{BufRead, BufReader, Read, Write};

use crate::error::{ClioError, Result};
use crate::models::{Memory, RememberInput};
use crate::repository;

/// Export all memories (or filtered by namespace) to JSONL.
pub fn export_jsonl(
    conn: &rusqlite::Connection,
    writer: &mut dyn Write,
    namespace: Option<&str>,
    include_archived: bool,
) -> Result<u32> {
    let query = crate::models::RecallQuery {
        namespace: namespace.map(String::from),
        include_archived,
        skip_access_tracking: true,
        limit: u32::MAX,
        ..Default::default()
    };

    let result = repository::recall(conn, &query)?;
    let mut count = 0;

    for item in &result.items {
        let line = serde_json::to_string(&item.memory)?;
        writer
            .write_all(line.as_bytes())
            .map_err(|e| ClioError::Export(format!("write failed: {e}")))?;
        writer
            .write_all(b"\n")
            .map_err(|e| ClioError::Export(format!("write failed: {e}")))?;
        count += 1;
    }

    Ok(count)
}

/// Import memories from JSONL. Uses upsert when source + source_ref are present.
pub fn import_jsonl(conn: &rusqlite::Connection, reader: &mut dyn Read) -> Result<ImportResult> {
    let buf = BufReader::new(reader);
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors = Vec::new();

    for (line_num, line_result) in buf.lines().enumerate() {
        let line = line_result.map_err(|e| ClioError::Import(format!("read error: {e}")))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Memory>(trimmed) {
            Ok(mem) => {
                let has_source = mem.source.is_some() && mem.source_ref.is_some();
                let input = RememberInput {
                    namespace: mem.namespace,
                    kind: mem.kind,
                    title: mem.title,
                    summary: mem.summary,
                    content: mem.content,
                    tags: mem.tags,
                    source: mem.source,
                    source_ref: mem.source_ref,
                    confidence: mem.confidence,
                    importance: mem.importance,
                    metadata: mem.metadata,
                    valid_from: mem.valid_from,
                    valid_until: mem.valid_until,
                    upsert: has_source,
                };

                match repository::remember(conn, &input, &crate::settings::Settings::default()) {
                    Ok(_) => imported += 1,
                    Err(e) => {
                        errors.push(format!("line {}: {e}", line_num + 1));
                        skipped += 1;
                    }
                }
            }
            Err(e) => {
                errors.push(format!("line {}: parse error: {e}", line_num + 1));
                skipped += 1;
            }
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        errors,
    })
}

/// Result of a JSONL import operation.
#[derive(Debug)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_preserves_access_and_events() {
        let conn = crate::db::open_in_memory().unwrap();
        let input =
            serde_json::from_value(serde_json::json!({"content":"export fixture"})).unwrap();
        let memory =
            crate::repository::remember(&conn, &input, &crate::settings::Settings::default())
                .unwrap();
        let before = crate::repository::get_raw(&conn, &memory.id).unwrap();
        let events = || {
            conn.query_row("SELECT COUNT(*) FROM memory_events", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
        };
        let count = events();
        super::export_jsonl(&conn, &mut Vec::new(), None, false).unwrap();
        let after = crate::repository::get_raw(&conn, &memory.id).unwrap();
        assert_eq!(before.access_count, after.access_count);
        assert_eq!(before.last_accessed_at, after.last_accessed_at);
        assert_eq!(count, events());
    }
}
