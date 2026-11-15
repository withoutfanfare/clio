//! Context assembly — build scoped context briefs for agent consumption.
//!
//! Combines recent, important, and kind-filtered memories into a structured
//! brief that agents can consume in a single call.

use std::fmt;
use std::str::FromStr;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{ClioError, Result};
use crate::models::{Memory, RecallQuery, SortOrder, now_utc};
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
    Handoff,
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
            Self::Handoff => "handoff",
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
            "handoff" => Ok(Self::Handoff),
            "custom" => Ok(Self::Custom),
            other => Err(ClioError::Validation(format!(
                "unknown context preset: '{other}'. Expected one of: project-brief, \
                 person-brief, decision-history, active-constraints, recent-activity, handoff, custom"
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

    /// Optional character budget for the whole brief. When set, sections are
    /// truncated greedily (in order) once the summed content length is reached,
    /// so briefs never balloon. Always keeps at least the first memory.
    #[serde(default)]
    pub char_budget: Option<u32>,

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
            char_budget: None,
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
        ContextPreset::Handoff => build_handoff(
            conn,
            &ns,
            request.query.as_deref(),
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

    // De-duplicate across sections (a decision tagged as a constraint can appear
    // in two presets' sections) and apply the optional character budget.
    let sections = dedup_and_budget(sections, request.char_budget);

    let total_memories_used: u32 = sections.iter().map(|s| s.items.len() as u32).sum();

    Ok(ContextBrief {
        namespace: ns.clone().unwrap_or_else(|| "global".to_string()),
        preset: request.preset.to_string(),
        sections,
        total_memories_used,
        generated_at: now_utc(),
    })
}

/// Approximate rendered length of a memory in a brief: title + preview.
fn brief_char_len(m: &Memory) -> usize {
    m.title.as_deref().map(str::len).unwrap_or(0) + m.summary.as_deref().unwrap_or(&m.content).len()
}

/// Drop cross-section duplicate memories (first occurrence wins) and, when a
/// `char_budget` is set, greedily drop items once the summed content length is
/// reached. At least the first memory is always kept. Section headings are
/// preserved (items may become empty) so the brief shape stays stable.
fn dedup_and_budget(
    sections: Vec<ContextSection>,
    char_budget: Option<u32>,
) -> Vec<ContextSection> {
    let budget = char_budget.map(|b| b as usize);
    let mut seen = std::collections::HashSet::new();
    let mut used = 0usize;
    let mut full = false;

    sections
        .into_iter()
        .map(|section| {
            let mut kept = Vec::new();
            for m in section.items {
                if full || !seen.insert(m.id.clone()) {
                    continue; // budget exhausted, or cross-section duplicate
                }
                if let Some(b) = budget {
                    let len = brief_char_len(&m);
                    if used > 0 && used + len > b {
                        full = true;
                        continue;
                    }
                    used += len;
                }
                kept.push(m);
            }
            ContextSection {
                heading: section.heading,
                items: kept,
            }
        })
        .collect()
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

/// Handoff brief — everything an agent (or human) needs to pick up a ticket
/// or topic: memories matching the query (tags are FTS-indexed, so memories
/// tagged `ticket:<id>` surface too), the namespace's active constraints, and
/// recent session receipts. The query is required.
fn build_handoff(
    conn: &Connection,
    ns: &Option<String>,
    query: Option<&str>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let query = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => q,
        None => {
            return Err(ClioError::Validation(
                "the handoff preset requires a query — a ticket id or topic, e.g. \"CAD-42\""
                    .into(),
            ));
        }
    };

    let reserved_constraints = u32::from(max_items >= 2);
    let reserved_receipts = u32::from(max_items >= 3);
    let mut remaining = max_items.saturating_sub(reserved_constraints + reserved_receipts);
    let relevant_limit = 12.min(remaining);
    remaining = remaining.saturating_sub(relevant_limit);
    let extra_constraints = 4.min(remaining);
    let constraint_limit = reserved_constraints + extra_constraints;
    remaining = remaining.saturating_sub(extra_constraints);
    let receipt_limit = reserved_receipts + 2.min(remaining);

    let mut relevant = recall_section(
        conn,
        "Directly Relevant",
        ns,
        None,
        Some(query),
        relevant_limit,
        include_links,
        scoring,
    )?;
    relevant.items.retain(|item| item.kind != "receipt");
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
    let receipts = recall_section(
        conn,
        "Recent Receipts",
        ns,
        Some("receipt"),
        Some(query),
        receipt_limit,
        include_links,
        scoring,
    )?;

    Ok(vec![relevant, constraints, receipts])
}

// ---------------------------------------------------------------------------
// Resume brief — evidence-backed "pick up where you left off" policy
// ---------------------------------------------------------------------------

/// Parameters for building a resume brief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRequest {
    /// Project namespace scope.
    #[serde(default)]
    pub namespace: Option<String>,

    /// Prompt/task context for the relevant-knowledge section. The knowledge
    /// section abstains entirely when this is absent.
    #[serde(default)]
    pub query: Option<String>,

    /// Session or topic scope: enables once-per-scope surfacing suppression
    /// and `surfaced` event recording.
    #[serde(default)]
    pub session_id: Option<String>,

    /// Maximum items across all sections.
    #[serde(default = "default_max_items")]
    pub max_items: u32,

    /// Character budget over the serialised item content (title + content +
    /// reason). At least one item is always kept.
    #[serde(default)]
    pub char_budget: Option<u32>,

    /// Temporal relevance scoring for knowledge recall.
    #[serde(skip)]
    pub scoring: Option<ScoringConfig>,

    /// The query's vector, from `fusion::query_embedding`. With it, relevant
    /// knowledge is fused keyword and semantic retrieval; without it,
    /// keywords alone.
    #[serde(skip)]
    pub query_embedding: Option<crate::fusion::QueryEmbedding>,

    /// Dormancy policy from `AttentionConfig`.
    #[serde(default)]
    pub dormant_days: u32,

    /// Age cap from `AttentionConfig`: open items untouched for longer than
    /// this many days stop surfacing automatically unless they carry a due
    /// date or reminder. `0` disables the cap.
    #[serde(default)]
    pub max_age_days: u32,

    /// Fixed evaluation time for tests; `None` means now.
    #[serde(default)]
    pub now: Option<String>,
}

impl Default for ResumeRequest {
    fn default() -> Self {
        Self {
            namespace: None,
            query: None,
            session_id: None,
            max_items: default_max_items(),
            char_budget: None,
            scoring: None,
            query_embedding: None,
            dormant_days: 0,
            max_age_days: 0,
            now: None,
        }
    }
}

/// One resume entry: bounded content plus the evidence for why it appears now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeItem {
    pub memory_id: String,
    pub kind: String,
    pub title: Option<String>,
    /// Bounded content: the summary when present, otherwise a truncated
    /// excerpt. This is exactly what the budget counts.
    pub content: String,
    pub source: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Attention status (`open`/`snoozed`) or `knowledge`/`activity`.
    pub state: String,
    /// Why this item appears now.
    pub reason: String,
}

/// A labelled group of resume items. Empty sections are omitted entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSection {
    pub heading: String,
    pub items: Vec<ResumeItem>,
}

/// The assembled resume brief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeBrief {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_delivery: Vec<ResumeDelivery>,
    pub namespace: String,
    pub sections: Vec<ResumeSection>,
    pub total_items: u32,
    pub generated_at: String,
}

/// Receipt retained by the caller until it has injected the selected memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeDelivery {
    pub memory_id: String,
    pub session_id: String,
    pub reason: String,
    pub attention_updated_at: Option<String>,
    #[serde(default)]
    pub memory_updated_at: Option<String>,
}

fn delivery_event(
    conn: &Connection,
    delivery: &ResumeDelivery,
) -> Result<crate::events::EventInput> {
    let memory = repository::get_raw(conn, &delivery.memory_id)?;
    let mut key = format!(
        "surfaced:{}:{}:{}",
        memory.id, delivery.session_id, delivery.reason
    );
    if let Some(updated) = &delivery.attention_updated_at {
        let mut attention = crate::attention::resolve_attention(conn, &memory.id)?;
        attention.updated_at = updated.clone();
        key = crate::attention::surfaced_key(&attention, &delivery.session_id, &delivery.reason);
    } else if let Some(updated) = &delivery.memory_updated_at {
        key.push(':');
        key.push_str(updated);
    }
    Ok(crate::events::EventInput {
        idempotency_key: Some(key),
        memory_id: Some(memory.id),
        namespace: Some(memory.namespace),
        session_id: Some(delivery.session_id.clone()),
        actor: Some("resume".into()),
        event_type: crate::events::EVENT_SURFACED.into(),
        reason: Some(delivery.reason.clone()),
        metadata: serde_json::json!({"memory_updated_at": delivery.memory_updated_at}),
        ..Default::default()
    })
}

/// Acknowledge only injected IDs from a deferred brief. Atomic and retry-safe.
pub fn acknowledge_resume(conn: &Connection, brief: &ResumeBrief, ids: &[String]) -> Result<()> {
    let mut events = Vec::new();
    for id in ids {
        if !brief
            .sections
            .iter()
            .any(|s| s.items.iter().any(|i| &i.memory_id == id))
        {
            return Err(ClioError::Validation(format!(
                "memory {id} is not in this brief"
            )));
        }
        let deliveries: Vec<_> = brief
            .pending_delivery
            .iter()
            .filter(|d| &d.memory_id == id)
            .collect();
        if deliveries.is_empty() {
            return Err(ClioError::Validation(format!(
                "memory {id} has no pending delivery"
            )));
        }
        for delivery in deliveries {
            // A receipt for a follow-up that has changed since is stale: the
            // new state was never shown, so it must not count as a delivery.
            if delivery.attention_updated_at.as_ref().is_some_and(|shown| {
                crate::attention::resolve_attention(conn, &delivery.memory_id)
                    .is_ok_and(|current| &current.updated_at != shown)
            }) {
                continue;
            }
            let mut event = match delivery_event(conn, delivery) {
                Ok(event) => event,
                Err(ClioError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            event.actor = Some("resume:ack".into());
            if delivery.attention_updated_at.is_none() {
                event.idempotency_key.as_mut().unwrap().push_str(":ack");
            }
            events.push(event);
        }
    }
    let tx = conn.unchecked_transaction()?;
    for event in events {
        crate::events::record_event(&tx, &event)?;
    }
    tx.commit()?;
    Ok(())
}

/// An undated task shown in this many sessions without anyone touching it
/// stops being shown automatically. Acting on it, or giving it a date,
/// brings it back.
pub const IGNORED_SESSION_LIMIT: usize = 20;

/// Upper bound on a resume item's excerpt when no summary exists.
const RESUME_EXCERPT_CHARS: usize = 400;

fn resume_excerpt(memory: &Memory) -> String {
    if let Some(summary) = memory.summary.as_deref() {
        if !summary.is_empty() {
            return summary.to_string();
        }
    }
    let mut content = memory.content.clone();
    if content.chars().count() > RESUME_EXCERPT_CHARS {
        content = content
            .chars()
            .take(RESUME_EXCERPT_CHARS)
            .collect::<String>()
            + "…";
    }
    content
}

fn resume_item_from_memory(memory: &Memory, state: &str, reason: String) -> ResumeItem {
    ResumeItem {
        memory_id: memory.id.clone(),
        kind: memory.kind.clone(),
        title: memory.title.clone(),
        content: resume_excerpt(memory),
        source: memory.source.clone(),
        created_at: memory.created_at.clone(),
        updated_at: (memory.updated_at != memory.created_at).then(|| memory.updated_at.clone()),
        state: state.to_string(),
        reason,
    }
}

/// Serialised weight of one item: exactly the fields the budget promises.
fn resume_item_len(item: &ResumeItem) -> usize {
    item.title.as_deref().map(str::len).unwrap_or(0) + item.content.len() + item.reason.len()
}

fn human_reason(item: &crate::attention::AttentionItem, reason: &str) -> String {
    match reason {
        "overdue" => match &item.due_at {
            Some(due) => format!("overdue: was due {due}"),
            None => "overdue".into(),
        },
        "reminder_due" => match &item.remind_at {
            Some(remind) => format!("reminder due since {remind}"),
            None => "reminder due".into(),
        },
        "project_session" => "you asked to be reminded at the next session in this project".into(),
        "dormant" => format!("untouched since {}", item.updated_at),
        other => other.to_string(),
    }
}

/// Build an untracked brief, prioritising task knowledge on prompts and one
/// recent receipt at session start. Attention and constraints reserve slots.
/// Static sections suppress acknowledged, unedited memories only on prompts;
/// selection-time records never suppress constraints or decisions.
pub fn build_resume_brief(conn: &Connection, request: &ResumeRequest) -> Result<ResumeBrief> {
    build_resume(conn, request, true)
}

/// Select a brief without recording delivery. A session is required for acknowledgement.
pub fn build_resume_brief_unrecorded(
    conn: &Connection,
    request: &ResumeRequest,
) -> Result<ResumeBrief> {
    if request
        .session_id
        .as_deref()
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err(ClioError::Validation(
            "deferred resume requires a session".into(),
        ));
    }
    build_resume(conn, request, false)
}

fn build_resume(conn: &Connection, request: &ResumeRequest, record: bool) -> Result<ResumeBrief> {
    use crate::attention;

    let now = request.now.clone().unwrap_or_else(now_utc);

    // An unresolved namespace means `global` — never an unscoped sweep of
    // every project's memories and actions.
    let namespace = request
        .namespace
        .clone()
        .filter(|ns| !ns.trim().is_empty())
        .unwrap_or_else(|| "global".to_string());

    // 1. Eligible open work (already once-per-scope suppressed by core).
    let mut eligible = attention::eligible(
        conn,
        &attention::EligibilityContext {
            namespace: Some(namespace.clone()),
            scope: request.session_id.clone(),
            now: now.clone(),
            dormant_days: request.dormant_days,
            max_age_days: request.max_age_days,
        },
    )?;

    eligible.sort_by_key(|e| {
        (
            e.reason.as_str() == "dormant",
            e.item
                .due_at
                .clone()
                .or(e.item.remind_at.clone())
                .unwrap_or_else(|| "9999".into()),
        )
    });
    let mut surfaced: Vec<(crate::attention::AttentionItem, String)> = Vec::new();
    let mut needs_attention = Vec::new();
    for entry in eligible {
        if ignored_too_long(conn, &entry.item)? {
            continue;
        }
        let Some(memory) = eligible_memory(conn, &entry.item.memory_id, &now)? else {
            continue;
        };
        let reason = human_reason(&entry.item, entry.reason.as_str());
        needs_attention.push(resume_item_from_memory(&memory, &entry.item.status, reason));
        surfaced.push((entry.item, entry.reason.as_str().to_string()));
    }

    // Without a session nothing rotates the list, so a cap would hide the
    // rest for good. `surfaced` is cut with it: an item dropped here must not
    // be recorded as shown if it reappears as task knowledge.
    if request.session_id.is_some() {
        needs_attention.truncate(3);
        surfaced.truncate(3);
    }

    // 2. Blocked / waiting items (open, waiting_on set, not already included).
    // Undated waits nobody has touched in weeks are stale, not blocking.
    // Explicit due dates and reminders are exempt from the age cap.
    let stale_before = attention::stale_before(&now, request.max_age_days);
    let mut waiting = Vec::new();
    for item in attention::list_attention(conn, Some(&namespace), Some(attention::STATUS_OPEN), 50)?
    {
        let Some(waiting_on) = item.waiting_on.clone() else {
            continue;
        };
        if item.due_at.is_none()
            && item.remind_at.is_none()
            && stale_before
                .as_deref()
                .is_some_and(|cutoff| item.updated_at.as_str() < cutoff)
        {
            continue;
        }
        if let Some(scope) = &request.session_id {
            if crate::events::event_exists(conn, &attention::surfaced_key(&item, scope, "waiting"))?
            {
                continue;
            }
        }
        if ignored_too_long(conn, &item)? {
            continue;
        }
        let Some(memory) = eligible_memory(conn, &item.memory_id, &now)? else {
            continue;
        };
        waiting.push(resume_item_from_memory(
            &memory,
            &item.status,
            format!("waiting on {waiting_on}"),
        ));
        surfaced.push((item, "waiting".into()));
    }

    let has_query = request
        .query
        .as_deref()
        .is_some_and(|q| !q.trim().is_empty());
    let suppression_scope = request.session_id.as_deref().filter(|_| has_query);

    // 3. Constraints: project first, plus a modest global prior.
    let mut constraints = resume_recall(
        conn,
        resume_query(
            Some(&namespace),
            Some("constraint"),
            None,
            6,
            &request.scoring,
        ),
        "active constraint in this project",
        suppression_scope,
        None,
    )?;
    if request
        .namespace
        .as_deref()
        .is_some_and(|ns| ns != "global")
    {
        constraints.extend(resume_recall(
            conn,
            resume_query(
                Some("global"),
                Some("constraint"),
                None,
                2,
                &request.scoring,
            ),
            "global constraint",
            suppression_scope,
            None,
        )?);
    }

    // 4. Recent decisions — "recent" means newest by creation date, so a
    // heavily accessed old decision cannot outrank last week's.
    let decisions = resume_recall(
        conn,
        RecallQuery {
            sort_by: Some(SortOrder::CreatedDesc),
            ..resume_query(
                Some(&namespace),
                Some("decision"),
                None,
                5,
                &request.scoring,
            )
        },
        "recent decision in this project",
        suppression_scope,
        None,
    )?;

    // 5. Prompt-relevant knowledge — abstains without a query.
    let mut knowledge = match request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        Some(query) => {
            // The query is usually a whole prompt, so match on any of its
            // meaningful terms; requiring every word would match nothing.
            // With a query vector, semantic results are fused in.
            resume_recall(
                conn,
                RecallQuery {
                    match_any_term: true,
                    ..resume_query(Some(&namespace), None, Some(query), 10, &request.scoring)
                },
                "matches task terms",
                suppression_scope,
                request.query_embedding.as_ref(),
            )?
        }
        None => Vec::new(),
    };

    // 6. Recent substantive activity (receipts).
    let mut activity = resume_recall(
        conn,
        RecallQuery {
            sort_by: Some(SortOrder::CreatedDesc),
            ..resume_query(Some(&namespace), Some("receipt"), None, 3, &request.scoring)
        },
        "recent session receipt",
        suppression_scope,
        None,
    )?
    .into_iter()
    .map(|mut item| {
        item.state = "activity".into();
        item
    })
    .collect::<Vec<_>>();

    // Keep attention reasons when the same memory also matches the task, and
    // keep a matching constraint in its own section, which has a reserved slot.
    knowledge.retain(|k| {
        !needs_attention
            .iter()
            .chain(waiting.iter())
            .chain(constraints.iter())
            .any(|a| a.memory_id == k.memory_id)
    });
    if !has_query {
        activity.truncate(1);
    }
    let ordered = if has_query {
        vec![
            ("Relevant knowledge", knowledge),
            ("Needs attention", needs_attention),
            ("Waiting on", waiting),
            ("Active constraints", constraints),
            ("Recent decisions", decisions),
            ("Recent activity", activity),
        ]
    } else {
        vec![
            ("Recent activity", activity),
            ("Needs attention", needs_attention),
            ("Waiting on", waiting),
            ("Active constraints", constraints),
            ("Recent decisions", decisions),
        ]
    };

    let sections =
        allocate_resume_budget(ordered, request.max_items, request.char_budget, !has_query);

    let mut pending_delivery = Vec::new();
    if let Some(scope) = &request.session_id {
        for section in &sections {
            for item in &section.items {
                let attention: Vec<_> = surfaced
                    .iter()
                    .filter(|(a, _)| a.memory_id == item.memory_id)
                    .collect();
                // The revision selected for the brief, not a fresh read: an
                // edit made since must not be acknowledged as shown.
                let memory_updated_at =
                    Some(item.updated_at.clone().unwrap_or_else(|| item.created_at.clone()));
                let deliveries: Vec<_> = if attention.is_empty() {
                    vec![ResumeDelivery {
                        memory_id: item.memory_id.clone(),
                        session_id: scope.clone(),
                        reason: section.heading.clone(),
                        attention_updated_at: None,
                        memory_updated_at,
                    }]
                } else {
                    attention
                        .into_iter()
                        .map(|(a, reason)| ResumeDelivery {
                            memory_id: item.memory_id.clone(),
                            session_id: scope.clone(),
                            reason: reason.clone(),
                            attention_updated_at: Some(a.updated_at.clone()),
                            memory_updated_at: memory_updated_at.clone(),
                        })
                        .collect()
                };
                for delivery in deliveries {
                    if record {
                        match delivery_event(conn, &delivery) {
                            Ok(event) => crate::events::log_event(conn, &event),
                            Err(e) => tracing::warn!("resume event tracking failed: {e}"),
                        }
                    } else {
                        pending_delivery.push(delivery);
                    }
                }
            }
        }
    }

    let total_items: u32 = sections.iter().map(|s| s.items.len() as u32).sum();
    Ok(ResumeBrief {
        pending_delivery,
        namespace,
        sections,
        total_items,
        generated_at: now,
    })
}

/// True when an undated attention item has been shown in at least
/// [`IGNORED_SESSION_LIMIT`] sessions since anyone last touched it.
fn ignored_too_long(conn: &Connection, item: &crate::attention::AttentionItem) -> Result<bool> {
    if item.due_at.is_some() || item.remind_at.is_some() {
        return Ok(false);
    }
    let sessions: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT session_id) FROM memory_events
         WHERE memory_id = ?1 AND event_type = 'surfaced' AND created_at >= ?2",
        rusqlite::params![item.memory_id, item.updated_at],
        |row| row.get(0),
    )?;
    Ok(sessions >= IGNORED_SESSION_LIMIT as i64)
}

/// Load a memory for resume inclusion, applying archive/expiry eligibility.
fn eligible_memory(conn: &Connection, memory_id: &str, now: &str) -> Result<Option<Memory>> {
    let memory = match repository::get_raw(conn, memory_id) {
        Ok(memory) => memory,
        Err(ClioError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(e),
    };
    if memory.archived_at.is_some() {
        return Ok(None);
    }
    if memory
        .valid_until
        .as_deref()
        .is_some_and(|until| until <= now)
    {
        return Ok(None);
    }
    Ok(Some(memory))
}

/// The untracked, expiry-aware query every resume section starts from.
fn resume_query(
    namespace: Option<&str>,
    kind: Option<&str>,
    query: Option<&str>,
    limit: u32,
    scoring: &Option<ScoringConfig>,
) -> RecallQuery {
    RecallQuery {
        query: query.map(String::from),
        namespace: namespace.map(String::from),
        kind: kind.map(String::from),
        exclude_expired: true,
        limit,
        scoring: scoring.clone(),
        skip_access_tracking: true,
        ..Default::default()
    }
}

/// Untracked recall wrapped into resume items with a shared reason. With an
/// `embedding`, the recall is fused and an item only the semantic leg found
/// says so.
fn resume_recall(
    conn: &Connection,
    mut recall_query: RecallQuery,
    reason: &str,
    session: Option<&str>,
    embedding: Option<&crate::fusion::QueryEmbedding>,
) -> Result<Vec<ResumeItem>> {
    let limit = recall_query.limit as usize;
    let knowledge = recall_query.query.is_some();
    // Two slots are not spent on one fact: a near-copy is skipped and the
    // next candidate takes its place.
    let mut copies = crate::embeddings::NearCopies::new(
        recall_query
            .scoring
            .as_ref()
            .filter(|_| knowledge)
            .map_or(0.0, |s| s.dedup_similarity),
    );
    let mut items = Vec::new();
    if knowledge {
        recall_query.limit = 50;
    }
    {
        let found: Vec<(Memory, &str)> = match embedding {
            Some(_) => crate::fusion::fused_recall(conn, &recall_query, embedding)?
                .into_iter()
                .map(|f| {
                    let why = if f.keyword {
                        reason
                    } else {
                        "similar in meaning to the task"
                    };
                    (f.item.memory, why)
                })
                .collect(),
            None => repository::recall(conn, &recall_query)?
                .items
                .into_iter()
                .map(|ri| (ri.memory, reason))
                .collect(),
        };
        for (m, reason) in found {
            if knowledge
                && (m.kind == "receipt"
                    || (m.source.as_deref() == Some(crate::consolidate::CONSOLIDATED_SOURCE)
                        && crate::consolidate::consolidation_is_stale(conn, &m.namespace)?
                            .unwrap_or(true)))
            {
                continue;
            }
            if let Some(scope) = session {
                if ["constraint", "decision"].contains(&m.kind.as_str()) && conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_events WHERE memory_id = ?1 AND session_id = ?2 AND event_type = 'surfaced' AND actor = 'resume:ack' AND created_at >= ?3 AND json_extract(metadata_json, '$.memory_updated_at') = ?3)",
                    rusqlite::params![m.id, scope, m.updated_at], |r| r.get::<_, bool>(0))? { continue; }
            }
            if copies.repeats(conn, &m)? {
                continue;
            }
            items.push(resume_item_from_memory(&m, "knowledge", reason.to_string()));
            if items.len() >= limit {
                return Ok(items);
            }
        }
    }
    Ok(items)
}

/// Deduplicate by memory ID (section priority order), guarantee one slot for
/// each non-empty critical section, fill remaining capacity in section order,
/// then apply the character budget over the serialised representation. Empty
/// sections are dropped so they release capacity rather than reserving it.
fn allocate_resume_budget(
    ordered: Vec<(&str, Vec<ResumeItem>)>,
    max_items: u32,
    char_budget: Option<u32>,
    session_start: bool,
) -> Vec<ResumeSection> {
    let critical: Vec<usize> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, (h, _))| {
            (["Needs attention", "Waiting on", "Active constraints"].contains(h)
                || (session_start && *h == "Recent activity"))
                .then_some(i)
        })
        .collect();

    // Cross-section dedup, priority order.
    let mut seen = std::collections::HashSet::new();
    let mut pools: Vec<(String, Vec<ResumeItem>)> = ordered
        .into_iter()
        .map(|(heading, items)| {
            let kept = items
                .into_iter()
                .filter(|item| seen.insert(item.memory_id.clone()))
                .collect();
            (heading.to_string(), kept)
        })
        .collect();

    // Slot allocation: one guaranteed slot per non-empty critical section,
    // then greedy fill in section order.
    let max_items = max_items.max(1) as usize;
    let mut take: Vec<usize> = vec![0; pools.len()];
    let mut used = 0;
    for (index, (_, items)) in pools
        .iter()
        .enumerate()
        .filter(|(i, _)| critical.contains(i))
    {
        if !items.is_empty() && used < max_items {
            take[index] = 1;
            used += 1;
        }
    }
    for (index, (_, items)) in pools.iter().enumerate() {
        while take[index] < items.len() && used < max_items {
            take[index] += 1;
            used += 1;
        }
    }

    // Character budget over what is actually serialised. Reserved critical
    // items (the first slot of each non-empty critical section) are funded
    // FIRST, so a verbose earlier section cannot starve the slots the
    // reservation contract promised. At least one item is always kept.
    let budget = char_budget.map(|b| b as usize);
    let mut chars_used = 0usize;
    let mut first_kept = false;

    // Budget pass order: reserved criticals, then everything else in
    // section-priority order.
    let mut funded: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut dropped: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut fund = |item: &ResumeItem| {
        if let Some(b) = budget {
            let len = resume_item_len(item);
            if first_kept && chars_used + len > b {
                dropped.insert(item.memory_id.clone());
                return;
            }
            chars_used += len;
        }
        first_kept = true;
        funded.insert(item.memory_id.clone());
    };
    for (index, (_, items)) in pools
        .iter()
        .enumerate()
        .filter(|(i, _)| critical.contains(i))
    {
        if take[index] > 0 {
            if let Some(item) = items.first() {
                fund(item);
            }
        }
    }
    for (index, (_, items)) in pools.iter().enumerate() {
        for (position, item) in items.iter().enumerate().take(take[index]) {
            let is_reserved_critical = critical.contains(&index) && position == 0;
            if is_reserved_critical {
                continue; // already funded in the first pass
            }
            fund(item);
        }
    }

    let mut sections = Vec::new();
    for (index, (heading, items)) in pools.iter_mut().enumerate() {
        let kept: Vec<ResumeItem> = items
            .drain(..)
            .take(take[index])
            .filter(|item| funded.contains(&item.memory_id))
            .collect();
        if !kept.is_empty() {
            sections.push(ResumeSection {
                heading: heading.clone(),
                items: kept,
            });
        }
    }
    sections
}

// ---------------------------------------------------------------------------
// Shared helper — run a recall query and wrap the result as a section
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
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
        exclude_expired: true,
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
        make_memory_with_tags(conn, ns, kind, content, Vec::new())
    }

    fn make_memory_with_tags(
        conn: &Connection,
        ns: &str,
        kind: &str,
        content: &str,
        tags: Vec<String>,
    ) -> Memory {
        repository::remember(
            conn,
            &RememberInput {
                namespace: ns.into(),
                kind: kind.into(),
                title: Some(format!("{kind}: {}", &content[..content.len().min(30)])),
                summary: None,
                content: content.into(),
                tags,
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
            ContextPreset::Handoff,
            ContextPreset::Custom,
        ];

        for preset in presets {
            let s = preset.to_string();
            let parsed: ContextPreset = s.parse().unwrap();
            assert_eq!(parsed, preset);
        }
    }

    #[test]
    fn handoff_preset_requires_query() {
        let conn = test_db();
        let request = ContextRequest {
            namespace: Some("project:test".into()),
            preset: ContextPreset::Handoff,
            ..Default::default()
        };
        let result = build_context(&conn, &request);
        assert!(result.is_err(), "handoff without a query must error");
    }

    #[test]
    fn handoff_brief_gathers_ticket_memories_constraints_and_receipts() {
        let conn = test_db();

        // Tagged with the ticket id only — the content never mentions "cad-42".
        // This pins the convention that tags are FTS-indexed, so a query for the
        // ticket id finds tag-only memories.
        repository::remember(
            &conn,
            &RememberInput {
                namespace: "project:test".into(),
                kind: "decision".into(),
                title: Some("Index approach".into()),
                summary: None,
                content: "Chose the composite index approach for the orders table.".into(),
                tags: vec!["ticket:cad-42".into()],
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
        .unwrap();

        make_memory(
            &conn,
            "project:test",
            "constraint",
            "Never edit applied migrations.",
        );
        make_memory_with_tags(
            &conn,
            "project:test",
            "receipt",
            "Implemented the index; left the backfill undone.",
            vec!["ticket:cad-42".into()],
        );
        make_memory_with_tags(
            &conn,
            "project:test",
            "receipt",
            "Worked on an unrelated ticket.",
            vec!["ticket:cad-99".into()],
        );

        let request = ContextRequest {
            namespace: Some("project:test".into()),
            preset: ContextPreset::Handoff,
            query: Some("CAD-42".into()),
            ..Default::default()
        };
        let brief = build_context(&conn, &request).unwrap();

        let headings: Vec<&str> = brief.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(
            headings,
            vec!["Directly Relevant", "Active Constraints", "Recent Receipts"]
        );
        assert!(
            brief.sections[0]
                .items
                .iter()
                .any(|m| m.content.contains("composite index")),
            "tag-only ticket memory must surface in Directly Relevant"
        );
        assert!(
            brief.sections[1]
                .items
                .iter()
                .any(|m| m.content.contains("migrations"))
        );
        assert!(brief.sections[2].items.iter().any(|m| m.kind == "receipt"));
    }

    #[test]
    fn handoff_budget_reserves_constraints_and_receipts_at_small_max_items() {
        let conn = test_db();
        make_memory(
            &conn,
            "project:test",
            "constraint",
            "Never edit applied migrations.",
        );
        make_memory_with_tags(
            &conn,
            "project:test",
            "receipt",
            "Did a thing.",
            vec!["ticket:cad-42".into()],
        );
        make_memory(
            &conn,
            "project:test",
            "note",
            "The CAD-42 index work is in progress.",
        );
        let request = ContextRequest {
            namespace: Some("project:test".into()),
            preset: ContextPreset::Handoff,
            query: Some("CAD-42".into()),
            max_items: 10,
            ..Default::default()
        };
        let brief = build_context(&conn, &request).unwrap();
        assert!(!brief.sections[1].items.is_empty());
        assert!(!brief.sections[2].items.is_empty());
        assert!(!brief.sections[0].items.is_empty());
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

    #[test]
    fn char_budget_truncates_brief() {
        let conn = test_db();
        // Each memory's content is ~40 chars; a 60-char budget fits one comfortably
        // and forces truncation before the rest.
        for i in 0..8 {
            make_memory(
                &conn,
                "global",
                "note",
                &format!("Memory content padding number {i:02} here"),
            );
        }

        let brief = build_context(
            &conn,
            &ContextRequest {
                preset: ContextPreset::RecentActivity,
                max_items: 20,
                char_budget: Some(60),
                ..Default::default()
            },
        )
        .unwrap();

        // At least one kept, but far fewer than the 8 available.
        assert!(brief.total_memories_used >= 1);
        assert!(brief.total_memories_used < 8);
    }

    #[test]
    fn dedup_removes_cross_section_duplicates() {
        let conn = test_db();
        let m = make_memory(&conn, "global", "decision", "Shared across two sections");

        // Same memory appears in two sections (e.g. a decision tagged constraint).
        let sections = vec![
            ContextSection {
                heading: "Decisions".into(),
                items: vec![m.clone()],
            },
            ContextSection {
                heading: "Constraints".into(),
                items: vec![m.clone()],
            },
        ];

        let out = dedup_and_budget(sections, None);
        let total: usize = out.iter().map(|s| s.items.len()).sum();
        assert_eq!(
            total, 1,
            "duplicate memory should appear once across sections"
        );
    }

    // -----------------------------------------------------------------------
    // Resume brief
    // -----------------------------------------------------------------------

    const NOW: &str = "2026-07-29T12:00:00Z";

    fn open_action(conn: &Connection, ns: &str, content: &str, due: Option<&str>) -> Memory {
        let memory = repository::remember(
            conn,
            &RememberInput {
                namespace: ns.into(),
                kind: "task".into(),
                title: Some(content.into()),
                summary: None,
                content: content.into(),
                tags: vec![],
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
        .unwrap();
        crate::attention::create_attention(
            conn,
            &crate::attention::AttentionInput {
                memory_id: memory.id.clone(),
                due_at: due.map(String::from),
                ..crate::attention::AttentionInput::default()
            },
        )
        .unwrap();
        memory
    }

    fn resume_request(ns: &str) -> ResumeRequest {
        ResumeRequest {
            namespace: Some(ns.into()),
            now: Some(NOW.into()),
            ..ResumeRequest::default()
        }
    }

    fn section<'b>(brief: &'b ResumeBrief, heading: &str) -> Option<&'b ResumeSection> {
        brief.sections.iter().find(|s| s.heading == heading)
    }

    fn surfaced_count(conn: &Connection, memory_id: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM memory_events WHERE event_type = 'surfaced' AND memory_id = ?1",
            [memory_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn resume_records_every_shown_memory_once_per_session() {
        let conn = test_db();
        let ns = "project:resume";
        let constraint = make_memory(&conn, ns, "constraint", "Never edit applied migrations");

        let mut request = resume_request(ns);
        request.session_id = Some("session-1".into());
        build_resume_brief(&conn, &request).unwrap();
        build_resume_brief(&conn, &request).unwrap();

        assert_eq!(surfaced_count(&conn, &constraint.id), 1);
    }

    #[test]
    fn resume_stops_showing_a_task_ignored_in_many_sessions() {
        let conn = test_db();
        let ns = "project:resume";
        let ignored = open_action(&conn, ns, "Reproduce the critical finding", None);
        conn.execute(
            "UPDATE attention_items SET trigger_kind = 'project-session' WHERE memory_id = ?1",
            [&ignored.id],
        )
        .unwrap();
        let overdue = open_action(&conn, ns, "Ship the fix", Some("2026-07-01T00:00:00Z"));

        let shown = |session: &str| {
            let mut request = resume_request(ns);
            request.session_id = Some(session.into());
            let brief = build_resume_brief(&conn, &request).unwrap();
            section(&brief, "Needs attention")
                .map(|s| s.items.iter().map(|i| i.memory_id.clone()).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        for n in 0..IGNORED_SESSION_LIMIT {
            assert!(shown(&format!("session-{n}")).contains(&ignored.id));
        }

        let after = shown("one-session-too-many");
        assert!(!after.contains(&ignored.id), "ignored task still shown");
        assert!(after.contains(&overdue.id), "dated work keeps showing");
    }

    #[test]
    fn resume_puts_overdue_actions_first_with_reasons() {
        let conn = test_db();
        let ns = "project:resume";
        let overdue = open_action(
            &conn,
            ns,
            "Verify the deployment",
            Some("2026-07-01T00:00:00Z"),
        );
        make_memory(&conn, ns, "constraint", "Never edit applied migrations");
        make_memory(&conn, ns, "decision", "We chose SQLite as the store");

        let brief = build_resume_brief(&conn, &resume_request(ns)).unwrap();

        assert_eq!(brief.sections[0].heading, "Needs attention");
        let first = &brief.sections[0].items[0];
        assert_eq!(first.memory_id, overdue.id);
        assert!(first.reason.contains("overdue"), "reason: {}", first.reason);
        assert!(first.reason.contains("2026-07-01"), "reason cites evidence");

        let constraints = section(&brief, "Active constraints").unwrap();
        assert!(constraints.items.iter().all(|i| !i.reason.is_empty()));
    }

    #[test]
    fn resume_includes_waiting_items_and_global_constraint_prior() {
        let conn = test_db();
        let ns = "project:resume";
        let blocked = repository::remember(
            &conn,
            &RememberInput {
                namespace: ns.into(),
                kind: "task".into(),
                title: Some("Key rotation".into()),
                summary: None,
                content: "Rotate the Atlas SSH key".into(),
                tags: vec![],
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
        .unwrap();
        crate::attention::create_attention(
            &conn,
            &crate::attention::AttentionInput {
                memory_id: blocked.id.clone(),
                waiting_on: Some("Marcus: rotation window".into()),
                ..crate::attention::AttentionInput::default()
            },
        )
        .unwrap();
        make_memory(&conn, "global", "constraint", "British English everywhere");

        let brief = build_resume_brief(&conn, &resume_request(ns)).unwrap();

        let waiting = section(&brief, "Waiting on").unwrap();
        assert_eq!(waiting.items[0].memory_id, blocked.id);
        assert!(waiting.items[0].reason.contains("Marcus"));

        let constraints = section(&brief, "Active constraints").unwrap();
        assert!(
            constraints
                .items
                .iter()
                .any(|i| i.reason == "global constraint"),
            "global preferences arrive with a modest prior"
        );
    }

    #[test]
    fn resume_waiting_age_cap_exempts_dates() {
        for (due_at, remind_at, visible_with_cap) in [
            (None, None, false),
            (Some("2026-08-01T00:00:00Z"), None, true),
            (None, Some("2026-08-01T00:00:00Z"), true),
            (
                Some("2026-08-01T00:00:00Z"),
                Some("2026-08-01T00:00:00Z"),
                true,
            ),
        ] {
            let conn = test_db();
            let ns = "project:resume";
            let memory = make_memory(&conn, ns, "task", "Await the release window");
            let item = crate::attention::create_attention(
                &conn,
                &crate::attention::AttentionInput {
                    memory_id: memory.id.clone(),
                    due_at: due_at.map(String::from),
                    remind_at: remind_at.map(String::from),
                    waiting_on: Some("Release approval".into()),
                    ..crate::attention::AttentionInput::default()
                },
            )
            .unwrap();
            conn.execute(
                "UPDATE attention_items SET updated_at = '2026-06-01T00:00:00Z' WHERE id = ?1",
                [&item.id],
            )
            .unwrap();

            for (max_age_days, visible) in [(14, visible_with_cap), (0, true)] {
                let request = ResumeRequest {
                    max_age_days,
                    ..resume_request(ns)
                };
                let brief = build_resume_brief(&conn, &request).unwrap();
                assert!(section(&brief, "Needs attention").is_none());
                let waiting = section(&brief, "Waiting on");
                assert_eq!(
                    waiting.is_some(),
                    visible,
                    "due_at={due_at:?}, remind_at={remind_at:?}, max_age_days={max_age_days}",
                );
                if let Some(waiting) = waiting {
                    assert_eq!(waiting.items.len(), 1);
                    assert_eq!(waiting.items[0].memory_id, memory.id);
                }
            }
        }
    }

    #[test]
    fn resume_knowledge_abstains_without_query_and_matches_with_one() {
        let conn = test_db();
        let ns = "project:resume";
        make_memory(
            &conn,
            ns,
            "fact",
            "The spool uses atomic renames for durability",
        );
        make_memory(
            &conn,
            ns,
            "fact",
            "Completely unrelated trivia about bananas",
        );

        let without = build_resume_brief(&conn, &resume_request(ns)).unwrap();
        assert!(section(&without, "Relevant knowledge").is_none());

        let mut request = resume_request(ns);
        request.query = Some("spool durability".into());
        let with = build_resume_brief(&conn, &request).unwrap();
        let knowledge = section(&with, "Relevant knowledge").unwrap();
        assert!(knowledge.items.iter().any(|i| i.content.contains("spool")));
        assert!(
            !knowledge
                .items
                .iter()
                .any(|i| i.content.contains("bananas")),
            "irrelevant memory is omitted"
        );
    }

    #[test]
    fn resume_caps_followups_orders_due_dates_and_carries_dates() {
        let conn = test_db();
        let ns = "project:resume";
        for day in [4, 2, 3, 1] {
            open_action(
                &conn,
                ns,
                &format!("Task {day}"),
                Some(&format!("2026-07-0{day}T00:00:00Z")),
            );
        }
        let receipt = make_memory(&conn, ns, "receipt", "Latest session");
        conn.execute(
            "UPDATE memories SET created_at = '2025-01-01T00:00:00Z' WHERE id = ?1",
            [&receipt.id],
        )
        .unwrap();
        let mut request = resume_request(ns);
        // Without a session nothing rotates the list, so nothing is cut.
        let brief = build_resume_brief(&conn, &request).unwrap();
        assert_eq!(section(&brief, "Needs attention").unwrap().items.len(), 4);
        request.session_id = Some("cap-session".into());
        let brief = build_resume_brief_unrecorded(&conn, &request).unwrap();
        let tasks = &section(&brief, "Needs attention").unwrap().items;
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].reason.contains("2026-07-01"));
        request.session_id = None;
        request.max_items = 1;
        let brief = build_resume_brief(&conn, &request).unwrap();
        let item = &brief.sections[0].items[0];
        assert_eq!(item.memory_id, receipt.id);
        assert_eq!(item.created_at, "2025-01-01T00:00:00Z");
        assert!(item.updated_at.is_some());
    }

    #[test]
    fn followup_cut_by_the_cap_is_not_recorded_as_shown() {
        let conn = test_db();
        let ns = "project:resume";
        for day in [1, 2, 3, 4] {
            open_action(
                &conn,
                ns,
                &format!("Renewal task {day}"),
                Some(&format!("2026-07-0{day}T00:00:00Z")),
            );
        }
        let mut request = resume_request(ns);
        request.session_id = Some("cap-session".into());
        request.query = Some("renewal task".into());
        let first = build_resume_brief(&conn, &request).unwrap();
        assert_eq!(section(&first, "Needs attention").unwrap().items.len(), 3);
        // The fourth was cut and shown only as task knowledge, so it is still
        // due in this session's next brief.
        let second = build_resume_brief(&conn, &request).unwrap();
        let due = &section(&second, "Needs attention").unwrap().items;
        assert!(due.iter().any(|i| i.reason.contains("2026-07-04")), "{due:?}");
    }

    #[test]
    fn matching_constraint_keeps_its_reserved_slot() {
        let conn = test_db();
        let ns = "project:resume";
        make_memory(&conn, ns, "fact", "The spool is durable across restarts");
        let constraint = make_memory(&conn, ns, "constraint", "Keep the spool on local disk");
        let mut request = resume_request(ns);
        request.query = Some("spool".into());
        request.max_items = 1;
        let brief = build_resume_brief(&conn, &request).unwrap();
        let kept = &section(&brief, "Active constraints").unwrap().items;
        assert_eq!(kept[0].memory_id, constraint.id);
    }

    #[test]
    fn stale_followup_receipt_is_not_recorded() {
        let conn = test_db();
        let ns = "project:resume";
        let task = open_action(&conn, ns, "Dated task", Some("2026-07-01T00:00:00Z"));
        let mut request = resume_request(ns);
        request.session_id = Some("late-session".into());
        let brief = build_resume_brief_unrecorded(&conn, &request).unwrap();
        // The receipt names the revision that was selected.
        assert_eq!(
            brief.pending_delivery[0].memory_updated_at.as_deref(),
            Some(task.updated_at.as_str())
        );
        conn.execute(
            "UPDATE attention_items SET updated_at = '2030-01-01T00:00:00Z' WHERE memory_id = ?1",
            [&task.id],
        )
        .unwrap();
        acknowledge_resume(&conn, &brief, std::slice::from_ref(&task.id)).unwrap();
        assert_eq!(surfaced_count(&conn, &task.id), 0);
    }

    #[test]
    fn constraints_survive_restart_lost_delivery_and_edits() {
        let conn = test_db();
        let ns = "project:resume";
        let m = make_memory(&conn, ns, "constraint", "Keep storage safe");
        conn.execute(
            "UPDATE memories SET updated_at = '2020-01-01T00:00:00Z' WHERE id = ?1",
            [&m.id],
        )
        .unwrap();
        let mut request = resume_request(ns);
        request.session_id = Some("safe-session".into());
        build_resume_brief(&conn, &request).unwrap(); // Lost selection-time output.
        request.query = Some("unmatched".into());
        assert!(
            section(
                &build_resume_brief(&conn, &request).unwrap(),
                "Active constraints"
            )
            .is_some()
        );
        let brief = build_resume_brief_unrecorded(&conn, &request).unwrap();
        acknowledge_resume(&conn, &brief, std::slice::from_ref(&m.id)).unwrap();
        assert!(
            section(
                &build_resume_brief_unrecorded(&conn, &request).unwrap(),
                "Active constraints"
            )
            .is_none()
        );
        request.query = None;
        assert!(
            section(
                &build_resume_brief_unrecorded(&conn, &request).unwrap(),
                "Active constraints"
            )
            .is_some()
        );
        request.query = Some("unmatched".into());
        conn.execute(
            "UPDATE memories SET updated_at = '2021-01-01T00:00:00Z' WHERE id = ?1",
            [&m.id],
        )
        .unwrap();
        acknowledge_resume(&conn, &brief, std::slice::from_ref(&m.id)).unwrap(); // Late retry of old revision.
        let edited = build_resume_brief_unrecorded(&conn, &request).unwrap();
        assert!(section(&edited, "Active constraints").is_some());
        acknowledge_resume(&conn, &edited, std::slice::from_ref(&m.id)).unwrap();
        assert!(
            section(
                &build_resume_brief_unrecorded(&conn, &request).unwrap(),
                "Active constraints"
            )
            .is_none()
        );
    }

    #[test]
    fn acknowledgement_skips_deleted_memories_and_records_all_attention_reasons() {
        let conn = test_db();
        let ns = "project:resume";
        let gone = make_memory(&conn, ns, "constraint", "Removed fixture");
        let due = open_action(
            &conn,
            ns,
            "Overdue and waiting",
            Some("2026-07-01T00:00:00Z"),
        );
        conn.execute(
            "UPDATE attention_items SET waiting_on = 'review' WHERE memory_id = ?1",
            [&due.id],
        )
        .unwrap();
        let mut request = resume_request(ns);
        request.session_id = Some("ack-reasons".into());
        let brief = build_resume_brief_unrecorded(&conn, &request).unwrap();
        conn.execute("DELETE FROM memories WHERE id = ?1", [&gone.id])
            .unwrap();
        acknowledge_resume(&conn, &brief, &[gone.id, due.id.clone()]).unwrap();
        let events = crate::events::list_events(&conn, &due.id, 20).unwrap();
        assert_eq!(
            events.iter().filter(|e| e.event_type == "surfaced").count(),
            2
        );
        assert!(
            build_resume_brief_unrecorded(&conn, &request)
                .unwrap()
                .sections
                .is_empty()
        );
    }

    #[test]
    fn knowledge_refill_is_bounded_to_fifty_candidates() {
        let conn = test_db();
        let ns = "project:resume";
        let oldest = make_memory(&conn, ns, "fact", "Atomic receipt oldest");
        conn.execute("UPDATE memories SET created_at = '2020-01-01T00:00:00Z', updated_at = '2020-01-01T00:00:00Z' WHERE id = ?1", [&oldest.id]).unwrap();
        for i in 0..50 {
            make_memory(&conn, ns, "receipt", &format!("Atomic receipt {i}"));
        }
        let items = resume_recall(
            &conn,
            RecallQuery {
                sort_by: Some(SortOrder::CreatedDesc),
                ..resume_query(Some(ns), None, Some("atomic"), 10, &None)
            },
            "test",
            None,
            None,
        )
        .unwrap();
        assert!(
            items.is_empty(),
            "do not scan beyond the fifty-candidate window"
        );
    }

    #[test]
    fn prompt_budget_reserves_a_constraint_slot() {
        let conn = test_db();
        let ns = "project:resume";
        make_memory(&conn, ns, "constraint", "Keep data safe");
        make_memory(&conn, ns, "fact", "Atomic durability");
        let mut request = resume_request(ns);
        request.query = Some("atomic".into());
        request.max_items = 2;
        let brief = build_resume_brief(&conn, &request).unwrap();
        assert_eq!(
            section(&brief, "Active constraints").unwrap().items.len(),
            1
        );
        assert_eq!(
            section(&brief, "Relevant knowledge").unwrap().items.len(),
            1
        );
    }

    #[test]
    fn explicit_reminders_precede_dormant_backlog() {
        let conn = test_db();
        let ns = "project:resume";
        for i in 0..4 {
            open_action(&conn, ns, &format!("Dormant {i}"), None);
        }
        conn.execute(
            "UPDATE attention_items SET updated_at = '2020-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
        let memory = make_memory(&conn, ns, "task", "Explicit reminder");
        crate::attention::create_attention(
            &conn,
            &crate::attention::AttentionInput {
                memory_id: memory.id.clone(),
                trigger: Some("project-session".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let mut request = resume_request(ns);
        request.dormant_days = 1;
        let brief = build_resume_brief(&conn, &request).unwrap();
        assert_eq!(
            section(&brief, "Needs attention").unwrap().items[0].memory_id,
            memory.id
        );
    }

    #[test]
    fn deferred_delivery_only_suppresses_acknowledged_items() {
        let conn = test_db();
        let ns = "project:resume";
        make_memory(&conn, ns, "constraint", "Keep data safe");
        open_action(&conn, ns, "Due task", Some("2026-07-01T00:00:00Z"));
        let mut request = resume_request(ns);
        request.session_id = Some("delivery-test".into());
        let brief = build_resume_brief_unrecorded(&conn, &request).unwrap();
        let again = build_resume_brief_unrecorded(&conn, &request).unwrap();
        assert_eq!(brief.total_items, again.total_items);
        let id = section(&brief, "Active constraints").unwrap().items[0]
            .memory_id
            .clone();
        acknowledge_resume(&conn, &brief, std::slice::from_ref(&id)).unwrap();
        acknowledge_resume(&conn, &brief, std::slice::from_ref(&id)).unwrap();
        assert_eq!(crate::events::list_events(&conn, &id, 10).unwrap().len(), 1);
        request.query = Some("unmatched".into());
        let after = build_resume_brief_unrecorded(&conn, &request).unwrap();
        assert!(section(&after, "Active constraints").is_none());
        assert!(section(&after, "Needs attention").is_some());
        let due = section(&brief, "Needs attention").unwrap().items[0]
            .memory_id
            .clone();
        acknowledge_resume(&conn, &brief, &[due]).unwrap();
        assert!(
            section(
                &build_resume_brief_unrecorded(&conn, &request).unwrap(),
                "Needs attention"
            )
            .is_none()
        );
        assert!(acknowledge_resume(&conn, &brief, &["unknown".into()]).is_err());
    }

    #[test]
    fn resume_prioritises_task_without_refilling_static_sections() {
        let conn = test_db();
        let ns = "project:resume";
        for i in 0..12 {
            make_memory(&conn, ns, "decision", &format!("Decision {i}"));
        }
        for i in 0..15 {
            make_memory(&conn, ns, "receipt", &format!("Atomic receipt {i}"));
        }
        make_memory(&conn, ns, "fact", "Atomic durability");
        let mut request = resume_request(ns);
        request.session_id = Some("refill".into());
        request.max_items = 20;
        request.query = Some("unmatched".into());
        let first = build_resume_brief_unrecorded(&conn, &request).unwrap();
        let ids: Vec<_> = first
            .sections
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.memory_id.clone()))
            .collect();
        acknowledge_resume(&conn, &first, &ids).unwrap();
        let second = build_resume_brief_unrecorded(&conn, &request).unwrap();
        assert!(section(&second, "Recent decisions").is_none());
        request.query = Some("Atomic".into());
        request.max_items = 1;
        let task = build_resume_brief(&conn, &request).unwrap();
        assert_eq!(task.sections[0].heading, "Relevant knowledge");
        assert_eq!(task.sections[0].items[0].kind, "fact");
        assert_eq!(task.sections[0].items[0].reason, "matches task terms");
    }

    #[test]
    fn resume_knowledge_matches_any_term_of_a_natural_language_query() {
        let conn = test_db();
        let ns = "project:resume";
        make_memory(
            &conn,
            ns,
            "fact",
            "The spool uses atomic renames for durability",
        );
        make_memory(
            &conn,
            ns,
            "fact",
            "Completely unrelated trivia about bananas",
        );

        let mut request = resume_request(ns);
        // "need" and "crash" appear in no memory: requiring every term would
        // match nothing, so this only passes under any-term matching.
        request.query = Some("why does the spool need atomic renames after a crash".into());
        let brief = build_resume_brief(&conn, &request).unwrap();
        let knowledge = section(&brief, "Relevant knowledge").unwrap();
        assert!(knowledge.items.iter().any(|i| i.content.contains("spool")));
        assert!(
            !knowledge
                .items
                .iter()
                .any(|i| i.content.contains("bananas")),
            "memories sharing no term are still omitted"
        );
    }

    #[test]
    fn resume_knowledge_fuses_in_what_only_meaning_finds() {
        let conn = test_db();
        let ns = "project:resume";
        let named = make_memory(&conn, ns, "fact", "The spool uses atomic renames");
        let meant = make_memory(&conn, ns, "fact", "Writes survive a power cut");
        let far = make_memory(&conn, ns, "fact", "Bananas are yellow");
        for (memory, vector) in [
            (&named, [0.0, 1.0]),
            (&meant, [1.0, 0.0]),
            (&far, [-1.0, 0.0]),
        ] {
            crate::embeddings::store_embedding(&conn, &memory.id, "test", 2, &vector).unwrap();
        }

        let mut request = resume_request(ns);
        request.query = Some("is the spool durable".into());
        request.scoring = Some(ScoringConfig::default());
        let reasons = |request: &ResumeRequest| -> Vec<(String, String)> {
            let brief = build_resume_brief(&conn, request).unwrap();
            section(&brief, "Relevant knowledge")
                .map(|s| {
                    s.items
                        .iter()
                        .map(|i| (i.memory_id.clone(), i.reason.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
        // Keywords alone: only the memory that shares a word.
        assert_eq!(
            reasons(&request),
            vec![(named.id.clone(), "matches task terms".to_string())]
        );

        request.query_embedding = Some(crate::fusion::QueryEmbedding {
            vector: vec![1.0, 0.0],
            model: "test".into(),
            deadline: None,
        });
        let fused = reasons(&request);
        assert_eq!(fused.len(), 2, "the unrelated memory stays out: {fused:?}");
        assert!(fused.contains(&(named.id, "matches task terms".to_string())));
        assert!(fused.contains(&(meant.id, "similar in meaning to the task".to_string())));
    }

    #[test]
    fn resume_knowledge_skips_near_copies_and_refills() {
        let conn = test_db();
        let ns = "project:resume";
        let first = make_memory(&conn, ns, "fact", "Deploys run the ship script");
        let copy = make_memory(&conn, ns, "fact", "Deploys always run the ship script");
        let other = make_memory(
            &conn,
            ns,
            "fact",
            "Deploys need a clean tree and a green build before anything else happens",
        );
        for (memory, vector) in [
            (&first, [1.0, 0.0]),
            (&copy, [0.99, 0.141]),
            (&other, [0.0, 1.0]),
        ] {
            crate::embeddings::store_embedding(&conn, &memory.id, "test", 2, &vector).unwrap();
        }
        let mut request = resume_request(ns);
        request.query = Some("deploys".into());
        request.max_items = 2;
        let shown = |request: &ResumeRequest| -> Vec<String> {
            let brief = build_resume_brief(&conn, request).unwrap();
            let knowledge = section(&brief, "Relevant knowledge").unwrap();
            knowledge
                .items
                .iter()
                .map(|i| i.memory_id.clone())
                .collect()
        };

        request.scoring = Some(ScoringConfig::default());
        let deduped = shown(&request);
        assert_eq!(deduped.len(), 2);
        assert!(deduped.contains(&other.id), "the freed slot is refilled");
        assert!(
            deduped.contains(&first.id) != deduped.contains(&copy.id),
            "one of the two copies, not both: {deduped:?}"
        );

        request.scoring = Some(ScoringConfig {
            dedup_similarity: 0.0,
            ..Default::default()
        });
        let kept = shown(&request);
        assert!(
            kept.contains(&first.id) && kept.contains(&copy.id),
            "{kept:?}"
        );
    }

    #[test]
    fn resume_decisions_are_ordered_newest_first_regardless_of_score() {
        let conn = test_db();
        let ns = "project:resume";
        let older = make_memory(&conn, ns, "decision", "Use SQLite for storage");
        let newer = make_memory(&conn, ns, "decision", "Route the Mac through Atlas");
        // Make the older decision look far more "relevant" to composite
        // scoring: heavily accessed and important, but created long ago.
        conn.execute(
            "UPDATE memories SET created_at = '2025-01-01T00:00:00Z', access_count = 50, \
             importance = 5 WHERE id = ?1",
            [&older.id],
        )
        .unwrap();
        conn.execute(
            "UPDATE memories SET created_at = '2026-07-28T00:00:00Z', importance = 1 \
             WHERE id = ?1",
            [&newer.id],
        )
        .unwrap();

        let brief = build_resume_brief(&conn, &resume_request(ns)).unwrap();
        let decisions = section(&brief, "Recent decisions").unwrap();
        let ids: Vec<&str> = decisions
            .items
            .iter()
            .map(|i| i.memory_id.as_str())
            .collect();
        assert_eq!(ids, vec![newer.id.as_str(), older.id.as_str()]);
    }

    #[test]
    fn resume_excludes_resolved_archived_and_expired_items() {
        let conn = test_db();
        let ns = "project:resume";

        let resolved = open_action(&conn, ns, "Already done", Some("2026-07-01T00:00:00Z"));
        crate::attention::complete(&conn, &resolved.id, None, None, None).unwrap();

        let archived = open_action(&conn, ns, "Archived loop", Some("2026-07-01T00:00:00Z"));
        repository::archive(&conn, &archived.id).unwrap();

        let expired = repository::remember(
            &conn,
            &RememberInput {
                namespace: ns.into(),
                kind: "constraint".into(),
                title: Some("Expired rule".into()),
                summary: None,
                content: "This constraint has lapsed".into(),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: Some("2000-01-01T00:00:00Z".into()),
                upsert: false,
            },
            &crate::settings::Settings::default(),
        )
        .unwrap();

        let brief = build_resume_brief(&conn, &resume_request(ns)).unwrap();
        let all_ids: Vec<&str> = brief
            .sections
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.memory_id.as_str()))
            .collect();
        assert!(!all_ids.contains(&resolved.id.as_str()));
        assert!(!all_ids.contains(&archived.id.as_str()));
        assert!(!all_ids.contains(&expired.id.as_str()));
    }

    #[test]
    fn small_budget_reserves_critical_sections() {
        let conn = test_db();
        let ns = "project:resume";
        open_action(&conn, ns, "Overdue one", Some("2026-07-01T00:00:00Z"));
        open_action(&conn, ns, "Overdue two", Some("2026-07-02T00:00:00Z"));
        make_memory(&conn, ns, "constraint", "Keep FTS in sync");
        make_memory(&conn, ns, "decision", "Rust for the core");
        make_memory(&conn, ns, "decision", "SQLite for storage");

        let mut request = resume_request(ns);
        request.max_items = 3;
        let brief = build_resume_brief(&conn, &request).unwrap();

        assert_eq!(brief.total_items, 3);
        let attention = section(&brief, "Needs attention").unwrap();
        assert!(!attention.items.is_empty(), "critical work wins the budget");
        let constraints = section(&brief, "Active constraints").unwrap();
        assert_eq!(
            constraints.items.len(),
            1,
            "constraints keep their reserved slot at small budgets"
        );
    }

    #[test]
    fn char_budget_counts_the_serialised_representation() {
        let conn = test_db();
        let ns = "project:resume";
        for index in 0..5 {
            make_memory(
                &conn,
                ns,
                "decision",
                &format!("Decision {index}: {}", "x".repeat(300)),
            );
        }

        let mut request = resume_request(ns);
        request.char_budget = Some(500);
        let brief = build_resume_brief(&conn, &request).unwrap();

        let total: usize = brief
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .map(resume_item_len)
            .sum();
        assert!(brief.total_items >= 1, "at least one item is always kept");
        assert!(
            total <= 500 || brief.total_items == 1,
            "budget applies to the serialised representation, used {total}"
        );
    }

    #[test]
    fn automatic_resume_leaves_access_ranking_unchanged() {
        let conn = test_db();
        let ns = "project:resume";
        let memory = make_memory(&conn, ns, "decision", "Untracked decision");

        let brief = build_resume_brief(&conn, &resume_request(ns)).unwrap();
        assert!(brief.total_items >= 1);

        let after = repository::get_raw(&conn, &memory.id).unwrap();
        assert_eq!(after.access_count, 0, "resume must not train ranking");
        assert!(after.last_accessed_at.is_none());

        // Deliberate recall stays tracked.
        repository::recall(
            &conn,
            &RecallQuery {
                namespace: Some(ns.into()),
                query: Some("Untracked".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let tracked = repository::get_raw(&conn, &memory.id).unwrap();
        assert_eq!(tracked.access_count, 1);
    }

    #[test]
    fn resume_surfaces_once_per_session_and_records_events() {
        let conn = test_db();
        let ns = "project:resume";
        let overdue = open_action(&conn, ns, "Surface me", Some("2026-07-01T00:00:00Z"));

        let mut request = resume_request(ns);
        request.session_id = Some("session-a".into());

        let first = build_resume_brief(&conn, &request).unwrap();
        assert!(section(&first, "Needs attention").is_some());
        let events = crate::events::list_events(&conn, &overdue.id, 10).unwrap();
        assert!(events.iter().any(|e| e.event_type == "surfaced"));

        // Same session: suppressed. New session: appears again.
        let repeat = build_resume_brief(&conn, &request).unwrap();
        assert!(section(&repeat, "Needs attention").is_none());
        request.session_id = Some("session-b".into());
        let fresh = build_resume_brief(&conn, &request).unwrap();
        assert!(section(&fresh, "Needs attention").is_some());
    }
}
