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
    pub namespace: String,
    pub sections: Vec<ResumeSection>,
    pub total_items: u32,
    pub generated_at: String,
}

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

/// Build a deterministic resume brief: eligible open work first, then blocked
/// items, constraints (with a modest global prior), recent decisions,
/// prompt-relevant knowledge and recent substantive activity.
///
/// Every item carries a `reason`. All reads are untracked — automatic
/// delivery never changes `access_count` or ranking age. When `session_id` is
/// set, included attention items record an idempotent `surfaced` event and are
/// suppressed on repeat requests in the same scope unless their state changes.
pub fn build_resume_brief(conn: &Connection, request: &ResumeRequest) -> Result<ResumeBrief> {
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
    let eligible = attention::eligible(
        conn,
        &attention::EligibilityContext {
            namespace: Some(namespace.clone()),
            scope: request.session_id.clone(),
            now: now.clone(),
            dormant_days: request.dormant_days,
            max_age_days: request.max_age_days,
        },
    )?;

    let mut surfaced: Vec<(crate::attention::AttentionItem, String)> = Vec::new();
    let mut needs_attention = Vec::new();
    for entry in eligible {
        let Some(memory) = eligible_memory(conn, &entry.item.memory_id, &now)? else {
            continue;
        };
        let reason = human_reason(&entry.item, entry.reason.as_str());
        needs_attention.push(resume_item_from_memory(&memory, &entry.item.status, reason));
        surfaced.push((entry.item, entry.reason.as_str().to_string()));
    }

    // 2. Blocked / waiting items (open, waiting_on set, not already included).
    // The same age cap applies as for eligibility: a wait nobody has touched
    // in weeks is stale, not blocking.
    let stale_before = attention::stale_before(&now, request.max_age_days);
    let mut waiting = Vec::new();
    for item in attention::list_attention(conn, Some(&namespace), Some(attention::STATUS_OPEN), 50)?
    {
        let Some(waiting_on) = item.waiting_on.clone() else {
            continue;
        };
        if stale_before
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
    )?;

    // 5. Prompt-relevant knowledge — abstains without a query.
    let knowledge = match request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        Some(query) => {
            // The query is usually a whole prompt, so match on any of its
            // meaningful terms; requiring every word would match nothing.
            let mut items = resume_recall(
                conn,
                RecallQuery {
                    match_any_term: true,
                    ..resume_query(Some(&namespace), None, Some(query), 10, &request.scoring)
                },
                &format!("matches the current task ({query})"),
            )?;
            items.retain(|i| i.kind != "receipt");
            // Never lead with stale derived truth: drop the consolidated
            // singleton when its namespace has moved past its watermark.
            let has_consolidated = items
                .iter()
                .any(|i| i.source.as_deref() == Some(crate::consolidate::CONSOLIDATED_SOURCE));
            if has_consolidated {
                let stale =
                    crate::consolidate::consolidation_is_stale(conn, &namespace)?.unwrap_or(true);
                if stale {
                    items.retain(|i| {
                        i.source.as_deref() != Some(crate::consolidate::CONSOLIDATED_SOURCE)
                    });
                }
            }
            items
        }
        None => Vec::new(),
    };

    // 6. Recent substantive activity (receipts).
    let activity = resume_recall(
        conn,
        resume_query(Some(&namespace), Some("receipt"), None, 3, &request.scoring),
        "recent session receipt",
    )?
    .into_iter()
    .map(|mut item| {
        item.state = "activity".into();
        item
    })
    .collect::<Vec<_>>();

    let ordered = vec![
        ("Needs attention", needs_attention),
        ("Waiting on", waiting),
        ("Active constraints", constraints),
        ("Recent decisions", decisions),
        ("Relevant knowledge", knowledge),
        ("Recent activity", activity),
    ];

    let sections = allocate_resume_budget(ordered, request.max_items, request.char_budget);

    // Record surfaced events only for attention items that made the cut.
    if let Some(scope) = &request.session_id {
        let included: std::collections::HashSet<&str> = sections
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.memory_id.as_str()))
            .collect();
        for (item, reason) in &surfaced {
            if included.contains(item.memory_id.as_str()) {
                attention::record_surfaced(conn, item, scope, reason, Some("resume"));
            }
        }
    }

    let total_items: u32 = sections.iter().map(|s| s.items.len() as u32).sum();
    Ok(ResumeBrief {
        namespace,
        sections,
        total_items,
        generated_at: now,
    })
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

/// Untracked recall wrapped into resume items with a shared reason.
fn resume_recall(
    conn: &Connection,
    recall_query: RecallQuery,
    reason: &str,
) -> Result<Vec<ResumeItem>> {
    let result = repository::recall(conn, &recall_query)?;
    Ok(result
        .items
        .into_iter()
        .map(|ri| resume_item_from_memory(&ri.memory, "knowledge", reason.to_string()))
        .collect())
}

/// Deduplicate by memory ID (section priority order), guarantee one slot for
/// each non-empty critical section, fill remaining capacity in section order,
/// then apply the character budget over the serialised representation. Empty
/// sections are dropped so they release capacity rather than reserving it.
fn allocate_resume_budget(
    ordered: Vec<(&str, Vec<ResumeItem>)>,
    max_items: u32,
    char_budget: Option<u32>,
) -> Vec<ResumeSection> {
    const CRITICAL: usize = 3; // needs attention, waiting, constraints

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
    for (index, (_, items)) in pools.iter().enumerate().take(CRITICAL) {
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
    for (index, (_, items)) in pools.iter().enumerate().take(CRITICAL) {
        if take[index] > 0 {
            if let Some(item) = items.first() {
                fund(item);
            }
        }
    }
    for (index, (_, items)) in pools.iter().enumerate() {
        for (position, item) in items.iter().enumerate().take(take[index]) {
            let is_reserved_critical = index < CRITICAL && position == 0;
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
