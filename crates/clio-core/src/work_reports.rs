//! Isolated direct-report proof: reports are observations, never acceptance authority.
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{ClioError, Result};

pub const DEFAULT_STALE_AFTER_SECS: i64 = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Waiting,
    Stopped,
    Implemented,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActor {
    Agent,
    User,
    Other,
    None,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Current,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunKey {
    pub source: String,
    pub run_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReport {
    pub project: String,
    pub task: String,
    pub task_title: String,
    pub source: String,
    pub session_id: String,
    pub run_id: String,
    pub sequence: i64,
    pub observed_at: i64,
    pub worktree: String,
    pub revision: String,
    pub state: State,
    pub summary: String,
    pub next_step: String,
    pub next_actor: NextActor,
    pub evidence: Vec<String>,
    pub evidence_status: EvidenceStatus,
    pub supersedes: Option<RunKey>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: i64,
    pub received_at: i64,
    pub report: WorkReport,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    NeedsUser,
    Waiting,
    Stopped,
    Implemented,
    ReportingMissing,
    Conflict,
    Accepted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunView {
    pub receipt: Receipt,
    pub superseded: bool,
    pub stale: bool,
    #[serde(default)]
    pub accepted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskView {
    pub project: String,
    pub task: String,
    pub task_title: String,
    pub state: TaskState,
    pub next_actor: NextActor,
    pub stale: bool,
    pub source_unavailable: bool,
    pub runs: Vec<RunView>,
    #[serde(default)]
    pub acceptances: Vec<Acceptance>,
    #[serde(default)]
    pub recommendation: Option<Recommendation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Overview {
    pub tasks: Vec<TaskView>,
}

/// A human decision about one immutable implementation receipt, not its whole task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    pub receipt_id: i64,
    pub scope: String,
    pub accepted_by: String,
    pub accepted_at: i64,
    pub evidence: Vec<String>,
}

/// Agent-prepared guidance; storing or copying it never registers or authorises work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    pub project: String,
    pub parent_task: String,
    pub task: String,
    pub task_title: String,
    pub reason: String,
    pub next_actor: NextActor,
    pub prompt: String,
    pub evidence: Vec<String>,
    pub checked_at: i64,
}

pub fn accept(conn: &Connection, decision: &Acceptance, now: i64) -> Result<Acceptance> {
    if !conn.is_autocommit() {
        return Err(ClioError::Validation("acceptance requires its own durable write".into()));
    }
    store_acceptance(conn, decision, now)
}

fn store_acceptance(conn: &Connection, decision: &Acceptance, now: i64) -> Result<Acceptance> {
    text("scope", &decision.scope, 240, true)?;
    text("accepted_by", &decision.accepted_by, 240, true)?;
    guidance_evidence(&decision.evidence)?;
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload FROM work_reports WHERE id=?1",
            [decision.receipt_id],
            |row| row.get(0),
        )
        .optional()?;
    let report: WorkReport =
        serde_json::from_str(&payload.ok_or_else(|| {
            ClioError::Validation("acceptance requires an existing receipt".into())
        })?)?;
    if report.state != State::Implemented
        || decision.accepted_at < report.observed_at
        || decision.accepted_at > now
    {
        return Err(ClioError::Validation(
            "acceptance requires an implemented receipt, valid time and its own durable write"
                .into(),
        ));
    }
    conn.execute("INSERT INTO work_acceptances(receipt_id,payload) VALUES (?1,?2) ON CONFLICT(receipt_id) DO NOTHING", params![decision.receipt_id,serde_json::to_string(decision)?])?;
    let stored: String = conn.query_row(
        "SELECT payload FROM work_acceptances WHERE receipt_id=?1",
        [decision.receipt_id],
        |row| row.get(0),
    )?;
    let stored: Acceptance = serde_json::from_str(&stored)?;
    if stored != *decision {
        return Err(ClioError::Conflict(
            "acceptance is immutable; this receipt already has a different decision".into(),
        ));
    }
    Ok(stored)
}

/// Accept only the unchanged report reviewed in the local dashboard.
/// The write lock keeps a concurrent checkpoint from overtaking the current-head check.
pub fn accept_current(conn: &Connection, receipt_id: i64, now: i64) -> Result<Acceptance> {
    if !conn.is_autocommit() {
        return Err(ClioError::Validation("acceptance requires its own durable transaction".into()));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let payload: Option<String> = conn.query_row(
            "SELECT p.payload FROM work_reports p WHERE p.id=?1
             AND p.sequence=(SELECT MAX(sequence) FROM work_reports newer WHERE newer.source=p.source AND newer.run_id=p.run_id)
             AND NOT EXISTS(SELECT 1 FROM work_runs successor WHERE successor.previous_source=p.source AND successor.previous_run_id=p.run_id)",
            [receipt_id], |row| row.get(0),
        ).optional()?;
        let payload = payload.ok_or_else(|| ClioError::Conflict("This report has changed or been handed over. Refresh and review the current report.".into()))?;
        let report: WorkReport = serde_json::from_str(&payload)?;
        if report.state != State::Implemented {
            return Err(ClioError::Validation("Only an implemented change can be accepted.".into()));
        }
        let existing: Option<String> = conn.query_row(
            "SELECT payload FROM work_acceptances WHERE receipt_id=?1", [receipt_id], |row| row.get(0),
        ).optional()?;
        if let Some(existing) = existing { return Ok(serde_json::from_str(&existing)?); }
        store_acceptance(conn, &Acceptance {
            receipt_id,
            scope: report.task_title,
            accepted_by: "Danny".into(),
            accepted_at: now,
            evidence: vec![format!("Accepted in the local dashboard after reviewing receipt {receipt_id}")],
        }, now)
    })();
    match result {
        Ok(decision) => {
            crate::db::finish_transaction(conn, true, "")?;
            Ok(decision)
        }
        Err(error) => {
            crate::db::rollback_transaction(conn, true, "");
            Err(error)
        }
    }
}

pub fn recommend(conn: &Connection, guidance: &Recommendation, now: i64) -> Result<Recommendation> {
    for (field, value) in [
        ("project", &guidance.project),
        ("parent_task", &guidance.parent_task),
        ("task", &guidance.task),
        ("task_title", &guidance.task_title),
    ] {
        text(field, value, 240, true)?;
    }
    text("reason", &guidance.reason, 1000, true)?;
    text("prompt", &guidance.prompt, 8000, true)?;
    guidance_evidence(&guidance.evidence)?;
    let exists = |task: &str| -> Result<bool> {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_runs WHERE project=?1 AND task=?2)",
            params![guidance.project, task],
            |row| row.get(0),
        )?)
    };
    if guidance.checked_at < 0
        || guidance.checked_at > now
        || guidance.next_actor == NextActor::None
        || !exists(&guidance.parent_task)?
        || exists(&guidance.task)?
    {
        return Err(ClioError::Validation("recommendation requires an existing parent, an unreported next task, an actor, valid time and its own durable write".into()));
    }
    conn.execute("INSERT INTO work_recommendations(project,parent_task,checked_at,payload) VALUES (?1,?2,?3,?4) ON CONFLICT(project,parent_task) DO UPDATE SET checked_at=excluded.checked_at,payload=excluded.payload WHERE excluded.checked_at > work_recommendations.checked_at", params![guidance.project,guidance.parent_task,guidance.checked_at,serde_json::to_string(guidance)?])?;
    let stored: String = conn.query_row(
        "SELECT payload FROM work_recommendations WHERE project=?1 AND parent_task=?2",
        params![guidance.project, guidance.parent_task],
        |row| row.get(0),
    )?;
    let stored: Recommendation = serde_json::from_str(&stored)?;
    if stored != *guidance {
        return Err(ClioError::Conflict(
            "recommendation is older than, or conflicts with, the stored guidance".into(),
        ));
    }
    Ok(stored)
}

fn guidance_evidence(evidence: &[String]) -> Result<()> {
    if evidence.is_empty() || evidence.len() > 32 {
        return Err(ClioError::Validation(
            "guidance requires 1 to 32 evidence references".into(),
        ));
    }
    for reference in evidence {
        text("evidence", reference, 2000, true)?;
    }
    Ok(())
}

/// Store a report and return its committed receipt. Caller-owned transactions are rejected.
pub fn report(conn: &Connection, report: &WorkReport, received_at: i64) -> Result<Receipt> {
    validate(report, received_at)?;
    if !conn.is_autocommit() {
        return Err(ClioError::Validation(
            "report requires its own durable transaction".into(),
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = store(conn, report, received_at);
    match result {
        Ok(receipt) => {
            crate::db::finish_transaction(conn, true, "")?;
            Ok(receipt)
        }
        Err(error) => {
            crate::db::rollback_transaction(conn, true, "");
            Err(error)
        }
    }
}

fn store(conn: &Connection, report: &WorkReport, received_at: i64) -> Result<Receipt> {
    let existing = receipt_at(conn, &report.source, &report.run_id, report.sequence)?;
    if let Some(receipt) = existing {
        return if receipt.report == *report {
            Ok(receipt)
        } else {
            Err(ClioError::Conflict(
                "update identity already contains a different payload".into(),
            ))
        };
    }
    if let Some(initial) = receipt_at(conn, &report.source, &report.run_id, 0)? {
        let first = initial.report;
        if (
            report.project.as_str(),
            report.task.as_str(),
            report.session_id.as_str(),
            report.worktree.as_str(),
        ) != (
            first.project.as_str(),
            first.task.as_str(),
            first.session_id.as_str(),
            first.worktree.as_str(),
        ) || (report.supersedes.is_some() && report.supersedes != first.supersedes)
        {
            return Err(ClioError::Conflict("run identity cannot be rebound".into()));
        }
    } else {
        if report.sequence != 0 {
            return Err(ClioError::Validation(
                "register the run with sequence zero first".into(),
            ));
        }
        if let Some(previous) = &report.supersedes {
            let prior = receipt_at(conn, &previous.source, &previous.run_id, 0)?
                .ok_or_else(|| ClioError::Validation("superseded run must already exist".into()))?;
            if prior.report.project != report.project || prior.report.task != report.task {
                return Err(ClioError::Conflict(
                    "handover must stay within the same project and task".into(),
                ));
            }
            let replaced: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM work_runs WHERE previous_source=?1 AND previous_run_id=?2)",
                params![previous.source, previous.run_id], |row| row.get(0))?;
            if replaced {
                return Err(ClioError::Conflict("run already has a successor".into()));
            }
        }
        conn.execute("INSERT INTO work_runs (source,run_id,project,task,session_id,worktree,previous_source,previous_run_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![report.source,report.run_id,report.project,report.task,report.session_id,report.worktree,
                report.supersedes.as_ref().map(|key| &key.source),report.supersedes.as_ref().map(|key| &key.run_id)])?;
    }
    conn.execute("INSERT INTO work_reports (source,run_id,sequence,observed_at,received_at,payload) VALUES (?1,?2,?3,?4,?5,?6)",
        params![report.source,report.run_id,report.sequence,report.observed_at,received_at,serde_json::to_string(report)?])?;
    Ok(Receipt {
        id: conn.last_insert_rowid(),
        received_at,
        report: report.clone(),
    })
}

fn receipt_at(
    conn: &Connection,
    source: &str,
    run_id: &str,
    sequence: i64,
) -> Result<Option<Receipt>> {
    let row: Option<(i64,i64,String)> = conn.query_row(
        "SELECT id,received_at,payload FROM work_reports WHERE source=?1 AND run_id=?2 AND sequence=?3",
        params![source,run_id,sequence], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    row.map(|(id, received_at, payload)| {
        Ok(Receipt {
            id,
            received_at,
            report: serde_json::from_str(&payload)?,
        })
    })
    .transpose()
}

/// Immutable history ordered by update sequence, including superseded runs.
pub fn history(conn: &Connection, source: &str, run_id: &str) -> Result<Vec<Receipt>> {
    let mut statement = conn.prepare("SELECT id,received_at,payload FROM work_reports WHERE source=?1 AND run_id=?2 ORDER BY sequence")?;
    let rows = statement.query_map(params![source, run_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, received_at, payload) = row?;
        Ok(Receipt {
            id,
            received_at,
            report: serde_json::from_str(&payload)?,
        })
    })
    .collect()
}

/// Scope an overview to an exact, explicit project identifier; no namespace inference.
pub fn overview_for_project(
    conn: &Connection,
    now: i64,
    stale_after_secs: i64,
    project: Option<&str>,
) -> Result<Overview> {
    if project.is_some_and(|project| project.trim().is_empty() || project.contains('\0')) {
        return Err(ClioError::Validation(
            "project must be nonempty without NUL".into(),
        ));
    }
    let mut result = overview(conn, now, stale_after_secs)?;
    if let Some(project) = project {
        result.tasks.retain(|task| task.project == project);
    }
    Ok(result)
}

/// Group one highest-sequence observation per run into unique project/task views.
pub fn overview(conn: &Connection, now: i64, stale_after_secs: i64) -> Result<Overview> {
    if now < 0 || stale_after_secs < 0 {
        return Err(ClioError::Validation(
            "overview times must be nonnegative".into(),
        ));
    }
    // ponytail: scan current run heads for this bounded proof; paginate by project if volume grows.
    let mut statement = conn.prepare("SELECT r.id,r.received_at,r.payload,
        EXISTS(SELECT 1 FROM work_runs successor WHERE successor.previous_source=r.source AND successor.previous_run_id=r.run_id)
        FROM work_reports r WHERE r.sequence=(SELECT MAX(sequence) FROM work_reports newer WHERE newer.source=r.source AND newer.run_id=r.run_id)
        ORDER BY r.source,r.run_id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, bool>(3)?,
        ))
    })?;
    let mut grouped: BTreeMap<(String, String), Vec<RunView>> = BTreeMap::new();
    for row in rows {
        let (id, received_at, payload, superseded) = row?;
        let report: WorkReport = serde_json::from_str(&payload)?;
        let stale = now.saturating_sub(report.observed_at) > stale_after_secs;
        grouped
            .entry((report.project.clone(), report.task.clone()))
            .or_default()
            .push(RunView {
                receipt: Receipt {
                    id,
                    received_at,
                    report,
                },
                superseded,
                stale,
                accepted: false,
            });
    }
    let mut acceptances: BTreeMap<(String, String), Vec<Acceptance>> = BTreeMap::new();
    let mut query = conn.prepare("SELECT r.project,r.task,a.payload FROM work_acceptances a JOIN work_reports p ON p.id=a.receipt_id JOIN work_runs r ON r.source=p.source AND r.run_id=p.run_id ORDER BY a.receipt_id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (project, task, payload) = row?;
        acceptances
            .entry((project, task))
            .or_default()
            .push(serde_json::from_str(&payload)?);
    }
    let mut recommendations = BTreeMap::new();
    let mut query = conn.prepare("SELECT payload FROM work_recommendations")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        let guidance: Recommendation = serde_json::from_str(&row?)?;
        if !grouped.contains_key(&(guidance.project.clone(), guidance.task.clone())) {
            recommendations.insert(
                (guidance.project.clone(), guidance.parent_task.clone()),
                guidance,
            );
        }
    }
    let tasks = grouped
        .into_iter()
        .map(|((project, task), mut runs)| {
            let acceptances = acceptances
                .remove(&(project.clone(), task.clone()))
                .unwrap_or_default();
            let recommendation = recommendations.remove(&(project.clone(), task.clone()));
            for run in &mut runs {
                run.accepted = acceptances
                    .iter()
                    .any(|decision| decision.receipt_id == run.receipt.id);
                if run.accepted {
                    run.stale = false;
                }
            }
            let active: Vec<_> = runs.iter().filter(|run| !run.superseded).collect();
            // Existing-only handovers and one successor per predecessor guarantee an active head.
            let pending: Vec<_> = active.iter().copied().filter(|run| !run.accepted).collect();
            let accepted = pending.is_empty();
            let first = &pending.first().unwrap_or(&active[0]).receipt.report;
            let stale = pending.iter().any(|run| run.stale);
            let source_unavailable = pending
                .iter()
                .any(|run| run.receipt.report.evidence_status == EvidenceStatus::Unavailable);
            let conflict = pending.iter().any(|run| {
                let report = &run.receipt.report;
                report.state != first.state
                    || report.next_actor != first.next_actor
                    || report.next_step != first.next_step
            });
            let state = if accepted {
                TaskState::Accepted
            } else if conflict {
                TaskState::Conflict
            } else {
                match first.state {
                    State::Running if stale => TaskState::ReportingMissing,
                    State::Running => TaskState::Running,
                    State::Waiting if first.next_actor == NextActor::User => TaskState::NeedsUser,
                    State::Waiting => TaskState::Waiting,
                    State::Stopped => TaskState::Stopped,
                    State::Implemented => TaskState::Implemented,
                }
            };
            TaskView {
                project,
                task,
                task_title: first.task_title.clone(),
                state,
                next_actor: if accepted {
                    NextActor::None
                } else if conflict {
                    NextActor::Other
                } else {
                    first.next_actor.clone()
                },
                stale,
                source_unavailable,
                runs,
                acceptances,
                recommendation,
            }
        })
        .collect();
    Ok(Overview { tasks })
}

fn validate(report: &WorkReport, received_at: i64) -> Result<()> {
    for (field, value) in [
        ("project", &report.project),
        ("task", &report.task),
        ("task_title", &report.task_title),
        ("source", &report.source),
        ("session_id", &report.session_id),
        ("run_id", &report.run_id),
        ("revision", &report.revision),
    ] {
        text(field, value, 240, true)?;
    }
    text("worktree", &report.worktree, 2000, true)?;
    text("summary", &report.summary, 1000, false)?;
    text("next_step", &report.next_step, 1000, false)?;
    if report.evidence.len() > 32 {
        return Err(ClioError::Validation(
            "at most 32 evidence references are allowed".into(),
        ));
    }
    for value in &report.evidence {
        text("evidence", value, 2000, true)?;
    }
    if let Some(key) = &report.supersedes {
        text("supersedes source", &key.source, 240, true)?;
        text("supersedes run_id", &key.run_id, 240, true)?;
    }
    if report.sequence < 0 || report.observed_at < 0 || received_at < report.observed_at {
        return Err(ClioError::Validation(
            "sequence and times must be nonnegative; observed_at cannot be in the future".into(),
        ));
    }
    Ok(())
}

fn text(field: &str, value: &str, max: usize, required: bool) -> Result<()> {
    if value.chars().count() > max || (required && value.trim().is_empty()) || value.contains('\0')
    {
        return Err(ClioError::Validation(format!(
            "{field} must be {} to {max} characters without NUL",
            if required { 1 } else { 0 }
        )));
    }
    Ok(())
}

/// Resolve a plain local evidence file; URLs, fragments and line references stay text.
pub fn local_evidence_path(worktree: &str, reference: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, Path};
    let root = Path::new(worktree);
    let path = Path::new(reference);
    if !root.is_absolute()
        || reference.is_empty()
        || reference.chars().any(|c| c.is_control() || ":#?%\\".contains(c))
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    fn supported(path: &Path) -> bool {
        matches!(path.extension().and_then(|s| s.to_str()).map(str::to_ascii_lowercase).as_deref(),
            Some("md" | "txt" | "log" | "json" | "csv" | "pdf" | "png" | "jpg" | "jpeg" | "webp" | "gif"))
    }
    if !supported(path) { return None; }
    let root = root.canonicalize().ok()?;
    if !root.is_dir() { return None; }
    let target = root.join(path).canonicalize().ok()?;
    (target.starts_with(&root) && target.is_file() && supported(&target)).then_some(target)
}

/// Bind opening to evidence in an immutable stored receipt, never a UI-supplied worktree.
pub fn evidence_path(conn: &Connection, receipt_id: i64, reference: &str) -> Result<std::path::PathBuf> {
    let payload: Option<String> = conn.query_row(
        "SELECT payload FROM work_reports WHERE id=?1", [receipt_id], |row| row.get(0),
    ).optional()?;
    let invalid = || ClioError::Validation("Evidence is not an existing supported file within this task worktree".into());
    let report: WorkReport = serde_json::from_str(&payload.ok_or_else(invalid)?)?;
    if !report.evidence.iter().any(|item| item == reference) { return Err(invalid()); }
    local_evidence_path(&report.worktree, reference).ok_or_else(invalid)
}
