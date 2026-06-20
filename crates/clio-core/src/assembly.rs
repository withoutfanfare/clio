//! Context assembly — build scoped context briefs for agent consumption.
//!
//! Combines recent, important, and kind-filtered memories into a structured
//! brief that agents can consume in a single call.

use std::fmt;
use std::str::FromStr;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{ClioError, Result};
use crate::models::{Memory, RecallQuery, now_utc};
use crate::repository;
use crate::settings::ScoringConfig;

// ---------------------------------------------------------------------------
// Preset enum
// ---------------------------------------------------------------------------

/// Predefined assembly presets for common context-gathering patterns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextPreset {
    ProjectBrief,
    PersonBrief,
    DecisionHistory,
    ActiveConstraints,
    RecentActivity,
    Custom,
}

impl fmt::Display for ContextPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::ProjectBrief => "project-brief",
            Self::PersonBrief => "person-brief",
            Self::DecisionHistory => "decision-history",
            Self::ActiveConstraints => "active-constraints",
            Self::RecentActivity => "recent-activity",
            Self::Custom => "custom",
        };
        f.write_str(s)
    }
}

impl FromStr for ContextPreset {
    type Err = ClioError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "project-brief" => Ok(Self::ProjectBrief),
            "person-brief" => Ok(Self::PersonBrief),
            "decision-history" => Ok(Self::DecisionHistory),
            "active-constraints" => Ok(Self::ActiveConstraints),
            "recent-activity" => Ok(Self::RecentActivity),
            "custom" => Ok(Self::Custom),
            other => Err(ClioError::Validation(format!(
                "unknown context preset: '{other}'. Expected one of: project-brief, \
                 person-brief, decision-history, active-constraints, recent-activity, custom"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Parameters for building a context brief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    /// Namespace scope. Uses detected/global if None.
    #[serde(default)]
    pub namespace: Option<String>,

    /// Which preset to use.
    #[serde(default = "default_preset")]
    pub preset: ContextPreset,

    /// FTS query for the Custom preset.
    #[serde(default)]
    pub query: Option<String>,

    /// Maximum memories to include across all sections.
    #[serde(default = "default_max_items")]
    pub max_items: u32,

    /// Whether to include linked memories in results.
    #[serde(default)]
    pub include_links: bool,

    /// Temporal relevance scoring config. When set, recall results are ranked
    /// by a composite score of recency, access frequency, and importance
    /// rather than plain chronological order.
    #[serde(skip)]
    pub scoring: Option<ScoringConfig>,
}

fn default_preset() -> ContextPreset {
    ContextPreset::ProjectBrief
}

fn default_max_items() -> u32 {
    20
}

impl Default for ContextRequest {
    fn default() -> Self {
        Self {
            namespace: None,
            preset: default_preset(),
            query: None,
            max_items: default_max_items(),
            include_links: false,
            scoring: None,
        }
    }
}

/// A labelled group of memories within a brief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSection {
    pub heading: String,
    pub items: Vec<Memory>,
}

/// The assembled context brief returned to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBrief {
    pub namespace: String,
    pub preset: String,
    pub sections: Vec<ContextSection>,
    pub total_memories_used: u32,
    pub generated_at: String,
}

// ---------------------------------------------------------------------------
// Assembly logic
// ---------------------------------------------------------------------------

/// Build a context brief by querying memories according to the given preset.
pub fn build_context(conn: &Connection, request: &ContextRequest) -> Result<ContextBrief> {
    let ns = request.namespace.clone();

    let scoring = request.scoring.clone();

    let sections = match request.preset {
        ContextPreset::ProjectBrief => build_project_brief(
            conn,
            &ns,
            request.max_items,
            request.include_links,
            &scoring,
        )?,
        ContextPreset::PersonBrief => build_person_brief(
            conn,
            &ns,
            request.max_items,
            request.include_links,
            &scoring,
        )?,
        ContextPreset::DecisionHistory => build_decision_history(
            conn,
            &ns,
            request.max_items,
            request.include_links,
            &scoring,
        )?,
        ContextPreset::ActiveConstraints => build_active_constraints(
            conn,
            &ns,
            request.max_items,
            request.include_links,
            &scoring,
        )?,
        ContextPreset::RecentActivity => build_recent_activity(
            conn,
            &ns,
            request.max_items,
            request.include_links,
            &scoring,
        )?,
        ContextPreset::Custom => build_custom(
            conn,
            &ns,
            request.query.as_deref(),
            request.max_items,
            request.include_links,
            &scoring,
        )?,
    };

    let total_memories_used: u32 = sections.iter().map(|s| s.items.len() as u32).sum();

    Ok(ContextBrief {
        namespace: ns.clone().unwrap_or_else(|| "global".to_string()),
        preset: request.preset.to_string(),
        sections,
        total_memories_used,
        generated_at: now_utc(),
    })
}

// ---------------------------------------------------------------------------
// Preset builders
// ---------------------------------------------------------------------------

fn build_project_brief(
    conn: &Connection,
    ns: &Option<String>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let decision_limit = 5.min(max_items);
    let constraint_limit = 5.min(max_items.saturating_sub(decision_limit));
    let recent_limit = max_items.saturating_sub(decision_limit + constraint_limit);

    let decisions = recall_section(
        conn,
        "Recent Decisions",
        ns,
        Some("decision"),
        None,
        decision_limit,
        include_links,
        scoring,
    )?;
    let constraints = recall_section(
        conn,
        "Active Constraints",
        ns,
        Some("constraint"),
        None,
        constraint_limit,
        include_links,
        scoring,
    )?;
    let recent = recall_section(
        conn,
        "Recent Activity",
        ns,
        None,
        None,
        recent_limit,
        include_links,
        scoring,
    )?;

    let mut sections = Vec::new();

    // Lead with the AI-curated consolidated memory for this namespace, if one
    // exists — it is the highest-signal summary of the whole project.
    if let Some(ns_str) = ns {
        if let Ok(Some(consolidated)) = crate::repository::get_by_source_ref(
            conn,
            crate::consolidate::CONSOLIDATED_SOURCE,
            ns_str,
        ) {
            sections.push(ContextSection {
                heading: "Consolidated Memory".to_string(),
                items: vec![consolidated],
            });
        }
    }

    sections.push(decisions);
    sections.push(constraints);
    sections.push(recent);
    Ok(sections)
}

fn build_person_brief(
    conn: &Connection,
    ns: &Option<String>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let fact_limit = 10.min(max_items);
    let recent_limit = max_items.saturating_sub(fact_limit);

    let facts = recall_section(
        conn,
        "Key Facts",
        ns,
        Some("fact"),
        None,
        fact_limit,
        include_links,
        scoring,
    )?;
    let recent = recall_section(
        conn,
        "Recent Notes",
        ns,
        None,
        None,
        recent_limit,
        include_links,
        scoring,
    )?;

    Ok(vec![facts, recent])
}

fn build_decision_history(
    conn: &Connection,
    ns: &Option<String>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let decisions = recall_section(
        conn,
        "Decisions",
        ns,
        Some("decision"),
        None,
        max_items,
        include_links,
        scoring,
    )?;
    Ok(vec![decisions])
}

fn build_active_constraints(
    conn: &Connection,
    ns: &Option<String>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let constraints = recall_section(
        conn,
        "Constraints",
        ns,
        Some("constraint"),
        None,
        max_items,
        include_links,
        scoring,
    )?;
    Ok(vec![constraints])
}

fn build_recent_activity(
    conn: &Connection,
    ns: &Option<String>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let recent = recall_section(
        conn,
        "Recent",
        ns,
        None,
        None,
        max_items,
        include_links,
        scoring,
    )?;
    Ok(vec![recent])
}

fn build_custom(
    conn: &Connection,
    ns: &Option<String>,
    query: Option<&str>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let results = recall_section(
        conn,
        "Search Results",
        ns,
        None,
        query,
        max_items,
        include_links,
        scoring,
    )?;
    Ok(vec![results])
}

// ---------------------------------------------------------------------------
// Shared helper — run a recall query and wrap the result as a section
// ---------------------------------------------------------------------------

fn recall_section(
    conn: &Connection,
    heading: &str,
    namespace: &Option<String>,
    kind: Option<&str>,
    query: Option<&str>,
    limit: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<ContextSection> {
    if limit == 0 {
        return Ok(ContextSection {
            heading: heading.to_string(),
            items: Vec::new(),
        });
    }

    let recall_query = RecallQuery {
        query: query.map(String::from),
        namespace: namespace.clone(),
        kind: kind.map(String::from),
        include_links,
        limit,
        scoring: scoring.clone(),
        ..Default::default()
    };

    let result = repository::recall(conn, &recall_query)?;
    let items: Vec<Memory> = result.items.into_iter().map(|ri| ri.memory).collect();

    Ok(ContextSection {
        heading: heading.to_string(),
        items,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::RememberInput;

    fn test_db() -> Connection {
        db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn make_memory(conn: &Connection, ns: &str, kind: &str, content: &str) -> Memory {
        repository::remember(
            conn,
            &RememberInput {
                namespace: ns.into(),
                kind: kind.into(),
                title: Some(format!("{kind}: {}", &content[..content.len().min(30)])),
                summary: None,
                content: content.into(),
                tags: Vec::new(),
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &crate::settings::Settings::default(),
        )
        .unwrap()
    }

    #[test]
    fn preset_round_trip() {
        let presets = vec![
            ContextPreset::ProjectBrief,
            ContextPreset::PersonBrief,
            ContextPreset::DecisionHistory,
            ContextPreset::ActiveConstraints,
            ContextPreset::RecentActivity,
            ContextPreset::Custom,
        ];

        for preset in presets {
            let s = preset.to_string();
            let parsed: ContextPreset = s.parse().unwrap();
            assert_eq!(parsed, preset);
        }
    }

    #[test]
    fn invalid_preset_returns_error() {
        let result = "bogus".parse::<ContextPreset>();
        assert!(result.is_err());
    }

    #[test]
    fn empty_db_returns_empty_brief() {
        let conn = test_db();
        let brief = build_context(&conn, &ContextRequest::default()).unwrap();
        assert_eq!(brief.total_memories_used, 0);
        assert!(!brief.sections.is_empty());
    }

    #[test]
    fn project_brief_groups_by_kind() {
        let conn = test_db();

        make_memory(
            &conn,
            "project:test",
            "decision",
            "Use Rust for the backend",
        );
        make_memory(
            &conn,
            "project:test",
            "constraint",
            "Must support offline use",
        );
        make_memory(&conn, "project:test", "note", "General project note");

        let brief = build_context(
            &conn,
            &ContextRequest {
                namespace: Some("project:test".into()),
                preset: ContextPreset::ProjectBrief,
                max_items: 20,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(brief.sections.len(), 3);
        assert_eq!(brief.sections[0].heading, "Recent Decisions");
        assert_eq!(brief.sections[1].heading, "Active Constraints");
        assert_eq!(brief.sections[2].heading, "Recent Activity");
        // Each section queries independently, so memories may appear in multiple sections.
        // 1 decision + 1 constraint + up to 3 recent (remaining budget of 10) = 5 total items.
        assert!(brief.total_memories_used >= 3);
    }

    #[test]
    fn person_brief_collects_facts() {
        let conn = test_db();

        make_memory(&conn, "person:danny", "fact", "Prefers British English");
        make_memory(&conn, "person:danny", "note", "Met at Rust meetup");

        let brief = build_context(
            &conn,
            &ContextRequest {
                namespace: Some("person:danny".into()),
                preset: ContextPreset::PersonBrief,
                max_items: 20,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(brief.sections.len(), 2);
        assert_eq!(brief.sections[0].heading, "Key Facts");
        assert!(brief.total_memories_used >= 2);
    }

    #[test]
    fn custom_preset_uses_query() {
        let conn = test_db();

        make_memory(
            &conn,
            "global",
            "note",
            "Rust is a systems programming language",
        );
        make_memory(&conn, "global", "note", "Python is great for scripting");

        let brief = build_context(
            &conn,
            &ContextRequest {
                preset: ContextPreset::Custom,
                query: Some("Rust".into()),
                max_items: 10,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(brief.sections.len(), 1);
        assert_eq!(brief.sections[0].heading, "Search Results");
        // Should find at least the Rust memory.
        assert!(!brief.sections[0].items.is_empty());
    }

    #[test]
    fn max_items_respected() {
        let conn = test_db();

        for i in 0..10 {
            make_memory(&conn, "global", "note", &format!("Memory number {i}"));
        }

        let brief = build_context(
            &conn,
            &ContextRequest {
                preset: ContextPreset::RecentActivity,
                max_items: 3,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(brief.total_memories_used <= 3);
    }
}
