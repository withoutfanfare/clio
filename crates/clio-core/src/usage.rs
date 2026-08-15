//! Provider token-usage accounting for checkpoint distillations.
//!
//! Every distillation call already returns [`crate::capture::CaptureUsage`];
//! this module persists it onto the checkpoint row so spend is a query rather
//! than an estimate. Recording is fire-and-forget: like access tracking, it
//! must never fail the parent operation.

use rusqlite::{Connection, params};

use crate::capture::CaptureUsage;
use crate::error::Result;

/// Best-effort: stamp a committed checkpoint with the model and token usage of
/// the distillation call that produced it. Failures are logged, never raised.
pub fn record_checkpoint_usage(
    conn: &Connection,
    checkpoint_id: &str,
    model: &str,
    usage: &CaptureUsage,
) {
    let outcome = conn.execute(
        "UPDATE session_checkpoints
         SET model = ?2, input_tokens = ?3, cached_input_tokens = ?4,
             output_tokens = ?5, reasoning_tokens = ?6
         WHERE id = ?1",
        params![
            checkpoint_id,
            model,
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            usage.reasoning_tokens,
        ],
    );
    if let Err(e) = outcome {
        tracing::warn!("failed to record usage for checkpoint {checkpoint_id}: {e}");
    }
}

/// One day's aggregated distillation usage.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UsageDay {
    /// UTC day, `YYYY-MM-DD`.
    pub day: String,
    /// Checkpoints with recorded usage.
    pub calls: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Checkpoints from before usage recording existed (or whose recording
    /// failed); their tokens are not in the totals.
    pub unrecorded_calls: u64,
}

/// Aggregate recorded checkpoint usage per UTC day, most recent first,
/// covering the last `days` UTC calendar days including today (at least 1) —
/// so `--days 1` is today only, never a rolling 24-hour window that splits
/// across two dates.
pub fn usage_by_day(conn: &Connection, days: u32) -> Result<Vec<UsageDay>> {
    let days = days.max(1);
    let mut stmt = conn.prepare_cached(
        "SELECT substr(created_at, 1, 10) AS day,
                COUNT(input_tokens),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(cached_input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(reasoning_tokens), 0),
                SUM(CASE WHEN input_tokens IS NULL THEN 1 ELSE 0 END)
         FROM session_checkpoints
         WHERE created_at >= datetime('now', 'start of day', ?1)
         GROUP BY day
         ORDER BY day DESC",
    )?;
    let rows = stmt.query_map(params![format!("-{} days", days - 1)], |row| {
        Ok(UsageDay {
            day: row.get(0)?,
            calls: row.get(1)?,
            input_tokens: row.get(2)?,
            cached_input_tokens: row.get(3)?,
            output_tokens: row.get(4)?,
            reasoning_tokens: row.get(5)?,
            unrecorded_calls: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::DistilledMemory;
    use crate::checkpoint::{CheckpointRequest, store_checkpoint};
    use crate::settings::Settings;

    fn checkpoint(conn: &Connection, cursor: i64) -> String {
        // Built via serde so optional request fields take their defaults and
        // this test does not chase every addition to the struct.
        let req: CheckpointRequest = serde_json::from_value(serde_json::json!({
            "source": "claude-session",
            "session_id": "session-usage",
            "cursor": cursor,
            "namespace_override": null,
            "default_namespace": "project:usage-test",
            "cwd": null,
            "branch": null,
            "ticket": null,
        }))
        .expect("valid request");
        let atom = DistilledMemory {
            content: format!("fact at cursor {cursor}"),
            kind: "fact".into(),
            title: format!("Fact {cursor}"),
            summary: String::new(),
            tags: vec!["test".into()],
            namespace: "project:usage-test".into(),
            importance: 3,
            confidence: 1.0,
            attention: None,
            resolves: None,
        };
        store_checkpoint(conn, &req, &[atom], &Settings::default())
            .expect("checkpoint failed")
            .checkpoint_id
    }

    #[test]
    fn records_and_aggregates_usage_by_day() {
        let conn = crate::db::open_in_memory().unwrap();

        let first = checkpoint(&conn, 10);
        let second = checkpoint(&conn, 20);
        record_checkpoint_usage(
            &conn,
            &first,
            "gpt-4.1",
            &CaptureUsage {
                input_tokens: 3_000,
                output_tokens: 500,
                reasoning_tokens: 0,
                cached_input_tokens: 1_100,
            },
        );
        record_checkpoint_usage(
            &conn,
            &second,
            "gpt-4.1",
            &CaptureUsage {
                input_tokens: 2_000,
                output_tokens: 300,
                reasoning_tokens: 7,
                cached_input_tokens: 0,
            },
        );
        // A third checkpoint with no recorded usage counts as unrecorded.
        checkpoint(&conn, 30);

        let days = usage_by_day(&conn, 7).unwrap();
        assert_eq!(days.len(), 1, "all rows share today's UTC day");
        let today = &days[0];
        assert_eq!(today.calls, 2);
        assert_eq!(today.input_tokens, 5_000);
        assert_eq!(today.cached_input_tokens, 1_100);
        assert_eq!(today.output_tokens, 800);
        assert_eq!(today.reasoning_tokens, 7);
        assert_eq!(today.unrecorded_calls, 1);
    }

    #[test]
    fn usage_window_counts_calendar_days_not_rolling_hours() {
        let conn = crate::db::open_in_memory().unwrap();
        let id = checkpoint(&conn, 10);
        record_checkpoint_usage(&conn, &id, "gpt-4.1", &CaptureUsage::default());
        // Move the checkpoint to two calendar days ago (any hour).
        conn.execute(
            "UPDATE session_checkpoints SET created_at = datetime('now', '-2 days')",
            [],
        )
        .unwrap();

        assert!(
            usage_by_day(&conn, 1).unwrap().is_empty(),
            "--days 1 is today only"
        );
        assert_eq!(
            usage_by_day(&conn, 3).unwrap().len(),
            1,
            "--days 3 reaches two calendar days back"
        );
    }

    #[test]
    fn recording_a_missing_checkpoint_never_errors() {
        let conn = crate::db::open_in_memory().unwrap();
        // Must not panic or fail the caller — fire-and-forget contract.
        record_checkpoint_usage(&conn, "no-such-id", "gpt-4.1", &CaptureUsage::default());
        assert!(usage_by_day(&conn, 7).unwrap().is_empty());
    }
}
