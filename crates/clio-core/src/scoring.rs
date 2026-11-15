//! Shared composite relevance multiplier.
//!
//! Mirrors the FTS recall scoring SQL in `repository.rs` so that semantic and
//! keyword recall rank by the same temporal/importance signal. Returns a
//! neutral `1.0` when decay is disabled, preserving the `decay_lambda = 0.0`
//! backwards-compatibility invariant.

use crate::models::Memory;
use crate::settings::ScoringConfig;
use time::OffsetDateTime;

/// Composite multiplier applied on top of a base relevance score (BM25 for
/// keyword recall, cosine similarity for semantic recall):
///
/// ```text
/// decay(age) * (1 + access_boost) * clamp(importance / 3)
/// ```
///
/// The importance term is clamped to [`importance_band`].
///
/// `age` decays from `COALESCE(last_accessed_at, updated_at)` — matching the
/// FTS SQL exactly. Returns `1.0` (neutral) when `decay_lambda <= 0.0`.
pub fn composite_multiplier(memory: &Memory, scoring: &ScoringConfig, now: OffsetDateTime) -> f64 {
    if scoring.decay_lambda <= 0.0 {
        return 1.0;
    }

    let reference = memory
        .last_accessed_at
        .as_deref()
        .unwrap_or(&memory.updated_at);
    let age_days =
        match OffsetDateTime::parse(reference, &time::format_description::well_known::Rfc3339) {
            // Clamp to >= 0 so a future-dated timestamp can't inflate the score.
            Ok(ts) => ((now - ts).as_seconds_f64() / 86_400.0).max(0.0),
            Err(_) => 0.0, // Unparseable timestamp: treat as no decay.
        };

    let decay = (-scoring.decay_lambda * age_days).exp();
    let access =
        1.0 + 0.5_f64.min(scoring.access_boost_weight * (1.0 + memory.access_count as f64).ln());
    let (low, high) = importance_band(scoring);
    let importance = (memory.importance as f64 / 3.0).clamp(low, high);

    decay * access * importance
}

/// The range the importance multiplier is held to: `1/x ..= x` for
/// `importance_clamp = x`. The default 3.0 contains every value `importance /
/// 3` can take, so it changes nothing; 1.0 makes importance neutral.
pub(crate) fn importance_band(scoring: &ScoringConfig) -> (f64, f64) {
    let high = if scoring.importance_clamp.is_finite() {
        scoring.importance_clamp.max(1.0)
    } else {
        3.0
    };
    (1.0 / high, high)
}

/// The importance term for the recall SQL, clamped exactly as
/// [`composite_multiplier`] clamps it.
pub(crate) fn importance_sql(scoring: &ScoringConfig) -> String {
    let (low, high) = importance_band(scoring);
    format!("max({low:.6}, min({high:.6}, CAST(m.importance AS REAL) / 3.0))")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::format_description::well_known::Rfc3339;

    fn mem_at(importance: i32, access_count: i32, timestamp: &str) -> Memory {
        Memory {
            id: "m".into(),
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "c".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            archived_at: None,
            created_at: timestamp.into(),
            updated_at: timestamp.into(),
            last_accessed_at: None,
            access_count,
        }
    }

    fn now_str(now: OffsetDateTime) -> String {
        now.format(&Rfc3339).unwrap()
    }

    #[test]
    fn decay_lambda_zero_is_neutral() {
        let s = ScoringConfig {
            decay_lambda: 0.0,
            access_boost_weight: 0.1,
            min_similarity: 0.0,
            ..Default::default()
        };
        let m = mem_at(5, 10, "2020-01-01T00:00:00Z");
        assert_eq!(composite_multiplier(&m, &s, OffsetDateTime::now_utc()), 1.0);
    }

    #[test]
    fn fresh_zero_access_importance_three_is_unit() {
        let s = ScoringConfig {
            decay_lambda: 0.01,
            access_boost_weight: 0.1,
            min_similarity: 0.0,
            ..Default::default()
        };
        let now = OffsetDateTime::now_utc();
        let m = mem_at(3, 0, &now_str(now));
        // age ~0 -> decay ~1, access 0 -> factor 1, importance 3/3 = 1.
        assert!((composite_multiplier(&m, &s, now) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn importance_five_scales_above_importance_one() {
        let s = ScoringConfig {
            decay_lambda: 0.01,
            access_boost_weight: 0.1,
            min_similarity: 0.0,
            ..Default::default()
        };
        let now = OffsetDateTime::now_utc();
        let hi = composite_multiplier(&mem_at(5, 0, &now_str(now)), &s, now);
        let lo = composite_multiplier(&mem_at(1, 0, &now_str(now)), &s, now);
        assert!(
            hi > lo,
            "importance 5 ({hi}) should outscore importance 1 ({lo})"
        );
    }

    #[test]
    fn importance_clamp_narrows_or_removes_the_importance_term() {
        let now = OffsetDateTime::now_utc();
        let at = |importance: i32, clamp: f64| {
            let s = ScoringConfig {
                decay_lambda: 0.01,
                access_boost_weight: 0.0,
                importance_clamp: clamp,
                ..Default::default()
            };
            composite_multiplier(&mem_at(importance, 0, &now_str(now)), &s, now)
        };
        // The default leaves the whole 0.33 to 1.67 range in place.
        assert!((at(1, 3.0) - 1.0 / 3.0).abs() < 1e-3);
        assert!((at(5, 3.0) - 5.0 / 3.0).abs() < 1e-3);
        // Narrow: held to 0.8 to 1.25, importance 3 untouched.
        assert!((at(1, 1.25) - 0.8).abs() < 1e-3);
        assert!((at(5, 1.25) - 1.25).abs() < 1e-3);
        assert!((at(3, 1.25) - 1.0).abs() < 1e-3);
        // Off: importance plays no part. Values below 1 mean off as well.
        assert!((at(1, 1.0) - at(5, 1.0)).abs() < 1e-9);
        assert!((at(1, 0.2) - at(5, 0.2)).abs() < 1e-9);
        // With decay disabled the multiplier stays neutral whatever the clamp.
        let neutral = ScoringConfig {
            decay_lambda: 0.0,
            importance_clamp: 1.25,
            ..Default::default()
        };
        assert_eq!(composite_multiplier(&mem_at(5, 0, &now_str(now)), &neutral, now), 1.0);
    }

    #[test]
    fn older_memory_decays_below_fresh() {
        let s = ScoringConfig {
            decay_lambda: 0.05,
            access_boost_weight: 0.0,
            min_similarity: 0.0,
            ..Default::default()
        };
        let now = OffsetDateTime::now_utc();
        let fresh = composite_multiplier(&mem_at(3, 0, &now_str(now)), &s, now);
        let old = composite_multiplier(&mem_at(3, 0, "2000-01-01T00:00:00Z"), &s, now);
        assert!(
            old < fresh,
            "older memory ({old}) should decay below fresh ({fresh})"
        );
    }
}
