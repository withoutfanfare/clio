//! Fail-closed, history-preserving repair transactions.
//!
//! Ordinary edits intentionally advance a memory's semantic `updated_at`.
//! Evidence-backed namespace and archive repairs are different: they correct
//! classification without pretending the remembered fact changed. This module
//! therefore owns a narrow manifest/apply/rollback path with complete
//! compare-and-swap validation and an immutable in-database journal.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{finish_transaction, rollback_transaction as rollback_db_transaction};
use crate::error::{ClioError, Result};
use crate::migrations;
use crate::models::now_utc;

pub const REPAIR_MANIFEST_SCHEMA_VERSION: u32 = 1;
const MAX_EVIDENCE_CHARS: usize = 2_000;

/// A proposed repair operation, before database state is captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepairAction {
    Move { namespace: String },
    Archive,
}

/// One evidence-backed intent supplied by an operator audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairIntent {
    pub memory_id: String,
    pub action: RepairAction,
    pub evidence: String,
}

/// Private input used to build a state-bound manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairPlan {
    pub intents: Vec<RepairIntent>,
}

/// The repair-relevant fields of a memory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MemoryState {
    pub id: String,
    pub namespace: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

/// Complete attention state, so an operational change cannot be overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionState {
    pub id: String,
    pub memory_id: String,
    pub namespace: String,
    pub status: String,
    pub owner: Option<String>,
    pub due_at: Option<String>,
    pub remind_at: Option<String>,
    pub trigger_kind: Option<String>,
    pub waiting_on: Option<String>,
    pub completion_condition: Option<String>,
    pub external_system: Option<String>,
    pub external_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

/// Exact stored edge state. Metadata remains JSON text to preserve byte-for-byte
/// compare-and-swap semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LinkState {
    pub from_memory_id: String,
    pub to_memory_id: String,
    pub relationship: String,
    pub metadata_json: String,
    pub created_at: String,
}

/// Before/after attention state associated with a target memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionChange {
    pub before: AttentionState,
    pub after: AttentionState,
}

/// One state-bound target in a repair manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairTarget {
    pub memory_id: String,
    pub action: RepairAction,
    pub evidence: String,
    pub before: MemoryState,
    pub after: MemoryState,
    pub attention: Option<AttentionChange>,
}

/// A deterministic manifest safe to inspect privately before applying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairManifest {
    pub schema_version: u32,
    pub transaction_id: String,
    pub digest: String,
    pub generated_at: String,
    pub quick_check: String,
    pub store_generation: i64,
    pub snapshot_digest: String,
    pub targets: Vec<RepairTarget>,
    pub touching_links: Vec<LinkState>,
    pub removed_links: Vec<LinkState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ManifestPayload<'a> {
    schema_version: u32,
    generated_at: &'a str,
    quick_check: &'a str,
    store_generation: i64,
    snapshot_digest: &'a str,
    targets: &'a [RepairTarget],
    touching_links: &'a [LinkState],
    removed_links: &'a [LinkState],
}

/// Result of applying or replaying a repair transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOutcome {
    pub transaction_id: String,
    pub replayed: bool,
    pub journal_entries: usize,
}

/// An immutable journal export suitable for a private rollback file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairJournalExport {
    pub transaction_id: String,
    pub kind: String,
    pub manifest_digest: String,
    pub forward_transaction_id: Option<String>,
    pub created_at: String,
    pub committed_at: String,
    pub entries: Vec<RepairJournalEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairJournalEntry {
    pub sequence: i64,
    pub entity_type: String,
    pub entity_key: String,
    pub operation: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub evidence_json: String,
}

/// Build a manifest at the current time.
pub fn build_manifest(conn: &Connection, plan: &RepairPlan) -> Result<RepairManifest> {
    build_manifest_at(conn, plan, &now_utc())
}

/// Build a manifest at an explicit timestamp. The fixed-time form makes the
/// deterministic contract directly testable.
pub fn build_manifest_at(
    conn: &Connection,
    plan: &RepairPlan,
    generated_at: &str,
) -> Result<RepairManifest> {
    validate_plan(plan)?;
    validate_timestamp(generated_at)?;

    let owns_transaction = conn.is_autocommit();
    if owns_transaction {
        conn.execute_batch("BEGIN")?;
    }

    let result = (|| -> Result<RepairManifest> {
        let quick_check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if quick_check != "ok" {
            return Err(ClioError::Storage(format!(
                "database quick_check failed: {quick_check}"
            )));
        }

        let store_generation = current_store_generation(conn)?;
        let all_memories = load_all_memory_states(conn)?;
        let memory_by_id: BTreeMap<_, _> = all_memories
            .iter()
            .cloned()
            .map(|state| (state.id.clone(), state))
            .collect();
        let all_links = load_all_links(conn)?;
        let target_ids: BTreeSet<_> = plan
            .intents
            .iter()
            .map(|intent| intent.memory_id.as_str())
            .collect();

        let mut targets = Vec::with_capacity(plan.intents.len());
        for intent in &plan.intents {
            let before = memory_by_id
                .get(&intent.memory_id)
                .cloned()
                .ok_or_else(|| ClioError::NotFound(intent.memory_id.clone()))?;
            let mut after = before.clone();
            match &intent.action {
                RepairAction::Move { namespace } => {
                    if before.namespace == *namespace {
                        return Err(ClioError::Validation(format!(
                            "memory {} is already in namespace {namespace}",
                            intent.memory_id
                        )));
                    }
                    after.namespace = namespace.clone();
                }
                RepairAction::Archive => {
                    if before.archived_at.is_some() {
                        return Err(ClioError::Validation(format!(
                            "memory {} is already archived",
                            intent.memory_id
                        )));
                    }
                    after.archived_at = Some(generated_at.to_string());
                }
            }

            let attention = load_attention_for_memory(conn, &intent.memory_id)?.map(|before| {
                let mut after_attention = before.clone();
                if matches!(intent.action, RepairAction::Move { .. }) {
                    after_attention.namespace = after.namespace.clone();
                }
                AttentionChange {
                    before,
                    after: after_attention,
                }
            });

            targets.push(RepairTarget {
                memory_id: intent.memory_id.clone(),
                action: intent.action.clone(),
                evidence: intent.evidence.clone(),
                before,
                after,
                attention,
            });
        }
        targets.sort_by(|left, right| left.memory_id.cmp(&right.memory_id));

        let touching_links: Vec<_> = all_links
            .iter()
            .filter(|link| {
                target_ids.contains(link.from_memory_id.as_str())
                    || target_ids.contains(link.to_memory_id.as_str())
            })
            .cloned()
            .collect();
        let removed_links = invalid_auto_links_after(&touching_links, &targets, &memory_by_id);

        let snapshot_digest = sha256_json(&(store_generation, &all_memories, &all_links))?;
        let mut manifest = RepairManifest {
            schema_version: REPAIR_MANIFEST_SCHEMA_VERSION,
            transaction_id: String::new(),
            digest: String::new(),
            generated_at: generated_at.to_string(),
            quick_check,
            store_generation,
            snapshot_digest,
            targets,
            touching_links,
            removed_links,
        };
        let digest = manifest_payload_digest(&manifest)?;
        manifest.transaction_id = format!("repair:{digest}");
        manifest.digest = digest;
        Ok(manifest)
    })();

    if owns_transaction {
        match result {
            Ok(manifest) => {
                finish_transaction(conn, true, "")?;
                Ok(manifest)
            }
            Err(error) => {
                rollback_db_transaction(conn, true, "");
                Err(error)
            }
        }
    } else {
        result
    }
}

/// Apply a complete manifest in one transaction. A repeated identical manifest
/// replays the committed result; every other stale or conflicting state aborts.
pub fn apply_manifest(conn: &Connection, manifest: &RepairManifest) -> Result<RepairOutcome> {
    verify_manifest(manifest)?;
    if !conn.is_autocommit() {
        return Err(ClioError::Conflict(
            "repair apply requires an outermost transaction".to_string(),
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = apply_manifest_in_transaction(conn, manifest);
    finish_or_rollback(conn, result)
}

/// Apply pending migrations and a reviewed manifest in one outer transaction.
///
/// This is the live operator path. A conflict after migration installation
/// removes both the new schema and every attempted repair mutation.
pub fn apply_manifest_with_pending_migrations(
    conn: &Connection,
    manifest: &RepairManifest,
) -> Result<RepairOutcome> {
    verify_manifest(manifest)?;
    if !conn.is_autocommit() {
        return Err(ClioError::Conflict(
            "repair apply requires an outermost transaction".to_string(),
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        migrations::run_pending_in_transaction(conn)?;
        apply_manifest_in_transaction(conn, manifest)
    })();
    finish_or_rollback(conn, result)
}

fn apply_manifest_in_transaction(
    conn: &Connection,
    manifest: &RepairManifest,
) -> Result<RepairOutcome> {
    if let Some((kind, digest, stored_manifest)) = conn
        .query_row(
            "SELECT kind, manifest_digest, manifest_json
             FROM repair_transactions WHERE id = ?1",
            [&manifest.transaction_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    {
        let supplied = serde_json::to_string(manifest)?;
        if kind == "repair" && digest == manifest.digest && stored_manifest == supplied {
            let rolled_back: bool = conn.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM repair_transactions
                    WHERE kind = 'rollback' AND forward_transaction_id = ?1
                )",
                [&manifest.transaction_id],
                |row| row.get(0),
            )?;
            if rolled_back {
                return Err(ClioError::Conflict(format!(
                    "repair transaction {} was rolled back and cannot be replayed",
                    manifest.transaction_id
                )));
            }
            verify_current_after_state(conn, manifest)?;
            return Ok(RepairOutcome {
                transaction_id: manifest.transaction_id.clone(),
                replayed: true,
                journal_entries: journal_entry_count(conn, &manifest.transaction_id)?,
            });
        }
        return Err(ClioError::Conflict(format!(
            "transaction identifier {} is already committed with different content",
            manifest.transaction_id
        )));
    }

    verify_current_before_state(conn, manifest)?;

    let committed_at = now_utc();
    let manifest_json = serde_json::to_string(manifest)?;
    let mut sequence = 0_i64;
    for target in &manifest.targets {
        let changed = conn.execute(
            "UPDATE memories SET namespace = ?1, archived_at = ?2
             WHERE id = ?3 AND namespace = ?4 AND updated_at = ?5
               AND archived_at IS ?6",
            params![
                target.after.namespace,
                target.after.archived_at,
                target.memory_id,
                target.before.namespace,
                target.before.updated_at,
                target.before.archived_at,
            ],
        )?;
        if changed != 1 {
            return Err(conflict("memory", &target.memory_id));
        }
        insert_journal_entry(
            conn,
            &manifest.transaction_id,
            sequence,
            journal_entry(
                "memory",
                &target.memory_id,
                "update",
                Some(&target.before),
                Some(&target.after),
                &serde_json::json!({"evidence": target.evidence}),
            )?,
        )?;
        sequence += 1;

        if let Some(attention) = &target.attention {
            if attention.before != attention.after {
                let changed = conn.execute(
                    "UPDATE attention_items SET namespace = ?1 WHERE id = ?2",
                    params![attention.after.namespace, attention.after.id],
                )?;
                if changed != 1 {
                    return Err(conflict("attention", &attention.before.id));
                }
                insert_journal_entry(
                    conn,
                    &manifest.transaction_id,
                    sequence,
                    journal_entry(
                        "attention",
                        &attention.before.id,
                        "update",
                        Some(&attention.before),
                        Some(&attention.after),
                        &serde_json::json!({"memory_id": target.memory_id}),
                    )?,
                )?;
                sequence += 1;
            }
        }
    }

    for link in &manifest.removed_links {
        let changed = conn.execute(
            "DELETE FROM memory_links
             WHERE from_memory_id = ?1 AND to_memory_id = ?2 AND relationship = ?3
               AND metadata_json = ?4 AND created_at = ?5",
            params![
                link.from_memory_id,
                link.to_memory_id,
                link.relationship,
                link.metadata_json,
                link.created_at,
            ],
        )?;
        if changed != 1 {
            return Err(conflict("link", &link_key(link)));
        }
        insert_journal_entry(
            conn,
            &manifest.transaction_id,
            sequence,
            journal_entry(
                "link",
                &link_key(link),
                "delete",
                Some(link),
                Option::<&LinkState>::None,
                &serde_json::json!({"reason": "invalid automatic link after repair"}),
            )?,
        )?;
        sequence += 1;
    }

    // The header closes the deferred-FK journal. Journal rows are inserted
    // first so a database trigger can reject any late append after commit.
    conn.execute(
        "INSERT INTO repair_transactions
         (id, kind, manifest_digest, forward_transaction_id, manifest_json, created_at, committed_at)
         VALUES (?1, 'repair', ?2, NULL, ?3, ?4, ?5)",
        params![
            manifest.transaction_id,
            manifest.digest,
            manifest_json,
            manifest.generated_at,
            committed_at,
        ],
    )?;

    Ok(RepairOutcome {
        transaction_id: manifest.transaction_id.clone(),
        replayed: false,
        journal_entries: sequence as usize,
    })
}

/// Conditionally invert one committed repair. Any drift in an affected memory,
/// attention row or touching link set aborts the complete rollback.
pub fn rollback_transaction(conn: &Connection, forward_id: &str) -> Result<RepairOutcome> {
    if !conn.is_autocommit() {
        return Err(ClioError::Conflict(
            "repair rollback requires an outermost transaction".to_string(),
        ));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = rollback_in_transaction(conn, forward_id);
    finish_or_rollback(conn, result)
}

fn rollback_in_transaction(conn: &Connection, forward_id: &str) -> Result<RepairOutcome> {
    let existing_rollback = conn
        .query_row(
            "SELECT id, manifest_digest FROM repair_transactions
             WHERE kind = 'rollback' AND forward_transaction_id = ?1",
            [forward_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;

    let (kind, manifest_json): (String, String) = conn
        .query_row(
            "SELECT kind, manifest_json FROM repair_transactions WHERE id = ?1",
            [forward_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| ClioError::NotFound(format!("repair transaction {forward_id}")))?;
    if kind != "repair" {
        return Err(ClioError::Validation(format!(
            "transaction {forward_id} is not a forward repair"
        )));
    }
    let manifest: RepairManifest = serde_json::from_str(&manifest_json)?;
    verify_manifest(&manifest)?;
    if let Some((id, _digest)) = existing_rollback {
        verify_current_rolled_back_state(conn, &manifest)?;
        return Ok(RepairOutcome {
            transaction_id: id.clone(),
            replayed: true,
            journal_entries: journal_entry_count(conn, &id)?,
        });
    }
    verify_current_after_state(conn, &manifest)?;

    let rollback_id = format!("rollback:{}", sha256_bytes(forward_id.as_bytes()));
    let committed_at = now_utc();
    let mut sequence = 0_i64;
    for target in &manifest.targets {
        let changed = conn.execute(
            "UPDATE memories SET namespace = ?1, archived_at = ?2
             WHERE id = ?3 AND namespace = ?4 AND updated_at = ?5
               AND archived_at IS ?6",
            params![
                target.before.namespace,
                target.before.archived_at,
                target.memory_id,
                target.after.namespace,
                target.after.updated_at,
                target.after.archived_at,
            ],
        )?;
        if changed != 1 {
            return Err(conflict("memory", &target.memory_id));
        }
        insert_journal_entry(
            conn,
            &rollback_id,
            sequence,
            journal_entry(
                "memory",
                &target.memory_id,
                "update",
                Some(&target.after),
                Some(&target.before),
                &serde_json::json!({"forward_transaction_id": forward_id}),
            )?,
        )?;
        sequence += 1;

        if let Some(attention) = &target.attention {
            if attention.before != attention.after {
                let changed = conn.execute(
                    "UPDATE attention_items SET namespace = ?1 WHERE id = ?2",
                    params![attention.before.namespace, attention.before.id],
                )?;
                if changed != 1 {
                    return Err(conflict("attention", &attention.before.id));
                }
                insert_journal_entry(
                    conn,
                    &rollback_id,
                    sequence,
                    journal_entry(
                        "attention",
                        &attention.before.id,
                        "update",
                        Some(&attention.after),
                        Some(&attention.before),
                        &serde_json::json!({"forward_transaction_id": forward_id}),
                    )?,
                )?;
                sequence += 1;
            }
        }
    }

    for link in &manifest.removed_links {
        conn.execute(
            "INSERT INTO memory_links
             (from_memory_id, to_memory_id, relationship, metadata_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                link.from_memory_id,
                link.to_memory_id,
                link.relationship,
                link.metadata_json,
                link.created_at,
            ],
        )?;
        insert_journal_entry(
            conn,
            &rollback_id,
            sequence,
            journal_entry(
                "link",
                &link_key(link),
                "insert",
                Option::<&LinkState>::None,
                Some(link),
                &serde_json::json!({"forward_transaction_id": forward_id}),
            )?,
        )?;
        sequence += 1;
    }

    conn.execute(
        "INSERT INTO repair_transactions
         (id, kind, manifest_digest, forward_transaction_id, manifest_json, created_at, committed_at)
         VALUES (?1, 'rollback', ?2, ?3, ?4, ?5, ?5)",
        params![
            rollback_id,
            manifest.digest,
            forward_id,
            manifest_json,
            committed_at,
        ],
    )?;

    Ok(RepairOutcome {
        transaction_id: rollback_id,
        replayed: false,
        journal_entries: sequence as usize,
    })
}

/// Export a committed transaction without exposing memory contents.
pub fn export_transaction(conn: &Connection, transaction_id: &str) -> Result<RepairJournalExport> {
    let header = conn
        .query_row(
            "SELECT id, kind, manifest_digest, forward_transaction_id, created_at, committed_at
             FROM repair_transactions WHERE id = ?1",
            [transaction_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| ClioError::NotFound(format!("repair transaction {transaction_id}")))?;
    let mut stmt = conn.prepare(
        "SELECT sequence, entity_type, entity_key, operation,
                before_json, after_json, evidence_json
         FROM repair_journal_entries
         WHERE transaction_id = ?1 ORDER BY sequence",
    )?;
    let entries = stmt
        .query_map([transaction_id], |row| {
            Ok(RepairJournalEntry {
                sequence: row.get(0)?,
                entity_type: row.get(1)?,
                entity_key: row.get(2)?,
                operation: row.get(3)?,
                before_json: row.get(4)?,
                after_json: row.get(5)?,
                evidence_json: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(RepairJournalExport {
        transaction_id: header.0,
        kind: header.1,
        manifest_digest: header.2,
        forward_transaction_id: header.3,
        created_at: header.4,
        committed_at: header.5,
        entries,
    })
}

/// Database-wide generation used to invalidate cross-process namespace caches.
pub fn current_store_generation(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT generation FROM memory_store_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn validate_plan(plan: &RepairPlan) -> Result<()> {
    if plan.intents.is_empty() {
        return Err(ClioError::Validation(
            "repair plan must contain at least one intent".to_string(),
        ));
    }
    let mut ids = BTreeSet::new();
    for intent in &plan.intents {
        if intent.memory_id.trim().is_empty() {
            return Err(ClioError::Validation(
                "repair memory_id must not be empty".to_string(),
            ));
        }
        if !ids.insert(intent.memory_id.as_str()) {
            return Err(ClioError::Validation(format!(
                "duplicate repair target {}",
                intent.memory_id
            )));
        }
        let evidence = intent.evidence.trim();
        if evidence.is_empty() || evidence.chars().count() > MAX_EVIDENCE_CHARS {
            return Err(ClioError::Validation(format!(
                "evidence for {} must contain 1 to {MAX_EVIDENCE_CHARS} characters",
                intent.memory_id
            )));
        }
        if let RepairAction::Move { namespace } = &intent.action {
            validate_namespace(namespace)?;
        }
    }
    Ok(())
}

fn validate_namespace(namespace: &str) -> Result<()> {
    let length = namespace.chars().count();
    if !(1..=120).contains(&length) || namespace.trim() != namespace {
        return Err(ClioError::Validation(
            "repair namespace must contain 1 to 120 non-padding characters".to_string(),
        ));
    }
    if namespace.chars().any(char::is_control) {
        return Err(ClioError::Validation(
            "repair namespace must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<()> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|_| ())
        .map_err(|_| ClioError::Validation("generated_at must be RFC3339".to_string()))
}

fn verify_manifest(manifest: &RepairManifest) -> Result<()> {
    if manifest.schema_version != REPAIR_MANIFEST_SCHEMA_VERSION {
        return Err(ClioError::Validation(format!(
            "unsupported repair manifest schema {}",
            manifest.schema_version
        )));
    }
    if manifest.quick_check != "ok" {
        return Err(ClioError::Validation(
            "repair manifest was not built from a healthy database".to_string(),
        ));
    }
    if manifest.targets.is_empty() {
        return Err(ClioError::Validation(
            "repair manifest must contain at least one target".to_string(),
        ));
    }
    let mut previous_id: Option<&str> = None;
    let mut target_ids = BTreeSet::new();
    for target in &manifest.targets {
        if previous_id.is_some_and(|previous| previous >= target.memory_id.as_str()) {
            return Err(ClioError::Validation(
                "repair targets must be unique and sorted by memory ID".to_string(),
            ));
        }
        previous_id = Some(&target.memory_id);
        target_ids.insert(target.memory_id.as_str());
        if target.memory_id != target.before.id || target.memory_id != target.after.id {
            return Err(ClioError::Validation(format!(
                "repair target {} has inconsistent memory identities",
                target.memory_id
            )));
        }
        let evidence_length = target.evidence.trim().chars().count();
        if !(1..=MAX_EVIDENCE_CHARS).contains(&evidence_length) {
            return Err(ClioError::Validation(format!(
                "repair target {} has invalid evidence",
                target.memory_id
            )));
        }
        if target.before.updated_at != target.after.updated_at {
            return Err(ClioError::Validation(format!(
                "repair target {} changes semantic updated_at",
                target.memory_id
            )));
        }
        match &target.action {
            RepairAction::Move { namespace } => {
                validate_namespace(namespace)?;
                if target.before.namespace == *namespace
                    || target.after.namespace != *namespace
                    || target.after.archived_at != target.before.archived_at
                {
                    return Err(ClioError::Validation(format!(
                        "repair target {} has an inconsistent move",
                        target.memory_id
                    )));
                }
            }
            RepairAction::Archive => {
                if target.before.archived_at.is_some()
                    || target.after.archived_at.as_deref() != Some(manifest.generated_at.as_str())
                    || target.after.namespace != target.before.namespace
                {
                    return Err(ClioError::Validation(format!(
                        "repair target {} has an inconsistent archive",
                        target.memory_id
                    )));
                }
            }
        }
        if let Some(attention) = &target.attention {
            let mut expected = attention.before.clone();
            if matches!(target.action, RepairAction::Move { .. }) {
                expected.namespace = target.after.namespace.clone();
            }
            if attention.before.memory_id != target.memory_id
                || attention.after.memory_id != target.memory_id
                || attention.after != expected
            {
                return Err(ClioError::Validation(format!(
                    "repair target {} has an inconsistent attention change",
                    target.memory_id
                )));
            }
        }
    }
    if !is_strictly_sorted(&manifest.touching_links) || !is_strictly_sorted(&manifest.removed_links)
    {
        return Err(ClioError::Validation(
            "repair link snapshots must be unique and sorted".to_string(),
        ));
    }
    if manifest.touching_links.iter().any(|link| {
        !target_ids.contains(link.from_memory_id.as_str())
            && !target_ids.contains(link.to_memory_id.as_str())
    }) {
        return Err(ClioError::Validation(
            "repair manifest contains a link that touches no target".to_string(),
        ));
    }
    let digest = manifest_payload_digest(manifest)?;
    if digest != manifest.digest || manifest.transaction_id != format!("repair:{digest}") {
        return Err(ClioError::Validation(
            "repair manifest digest or transaction identifier is invalid".to_string(),
        ));
    }
    let removed: BTreeSet<_> = manifest.removed_links.iter().collect();
    let touching: BTreeSet<_> = manifest.touching_links.iter().collect();
    if !removed.is_subset(&touching) {
        return Err(ClioError::Validation(
            "repair manifest removes a link outside its touching-link snapshot".to_string(),
        ));
    }
    if manifest
        .removed_links
        .iter()
        .any(|link| link.relationship != "auto:relates_to")
    {
        return Err(ClioError::Validation(
            "repair manifest may remove only auto:relates_to links".to_string(),
        ));
    }
    Ok(())
}

fn is_strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn manifest_payload_digest(manifest: &RepairManifest) -> Result<String> {
    sha256_json(&ManifestPayload {
        schema_version: manifest.schema_version,
        generated_at: &manifest.generated_at,
        quick_check: &manifest.quick_check,
        store_generation: manifest.store_generation,
        snapshot_digest: &manifest.snapshot_digest,
        targets: &manifest.targets,
        touching_links: &manifest.touching_links,
        removed_links: &manifest.removed_links,
    })
}

fn sha256_json(value: &impl Serialize) -> Result<String> {
    Ok(sha256_bytes(&serde_json::to_vec(value)?))
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn load_all_memory_states(conn: &Connection) -> Result<Vec<MemoryState>> {
    let mut stmt =
        conn.prepare("SELECT id, namespace, updated_at, archived_at FROM memories ORDER BY id")?;
    Ok(stmt
        .query_map([], |row| {
            Ok(MemoryState {
                id: row.get(0)?,
                namespace: row.get(1)?,
                updated_at: row.get(2)?,
                archived_at: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

fn load_memory_state(conn: &Connection, id: &str) -> Result<Option<MemoryState>> {
    Ok(conn
        .query_row(
            "SELECT id, namespace, updated_at, archived_at FROM memories WHERE id = ?1",
            [id],
            |row| {
                Ok(MemoryState {
                    id: row.get(0)?,
                    namespace: row.get(1)?,
                    updated_at: row.get(2)?,
                    archived_at: row.get(3)?,
                })
            },
        )
        .optional()?)
}

fn load_attention_for_memory(conn: &Connection, memory_id: &str) -> Result<Option<AttentionState>> {
    Ok(conn
        .query_row(
            "SELECT id, memory_id, namespace, status, owner, due_at, remind_at,
                    trigger_kind, waiting_on, completion_condition, external_system,
                    external_ref, created_at, updated_at, resolved_at
             FROM attention_items WHERE memory_id = ?1",
            [memory_id],
            |row| {
                Ok(AttentionState {
                    id: row.get(0)?,
                    memory_id: row.get(1)?,
                    namespace: row.get(2)?,
                    status: row.get(3)?,
                    owner: row.get(4)?,
                    due_at: row.get(5)?,
                    remind_at: row.get(6)?,
                    trigger_kind: row.get(7)?,
                    waiting_on: row.get(8)?,
                    completion_condition: row.get(9)?,
                    external_system: row.get(10)?,
                    external_ref: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    resolved_at: row.get(14)?,
                })
            },
        )
        .optional()?)
}

fn load_all_links(conn: &Connection) -> Result<Vec<LinkState>> {
    let mut stmt = conn.prepare(
        "SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
         FROM memory_links
         ORDER BY from_memory_id, to_memory_id, relationship",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(LinkState {
                from_memory_id: row.get(0)?,
                to_memory_id: row.get(1)?,
                relationship: row.get(2)?,
                metadata_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

fn load_touching_links(conn: &Connection, target_ids: &BTreeSet<&str>) -> Result<Vec<LinkState>> {
    Ok(load_all_links(conn)?
        .into_iter()
        .filter(|link| {
            target_ids.contains(link.from_memory_id.as_str())
                || target_ids.contains(link.to_memory_id.as_str())
        })
        .collect())
}

fn verify_current_before_state(conn: &Connection, manifest: &RepairManifest) -> Result<()> {
    let store_generation = current_store_generation(conn)?;
    let all_memories = load_all_memory_states(conn)?;
    let all_links = load_all_links(conn)?;
    let snapshot_digest = sha256_json(&(store_generation, &all_memories, &all_links))?;
    if store_generation != manifest.store_generation || snapshot_digest != manifest.snapshot_digest
    {
        return Err(ClioError::Conflict(
            "repair store snapshot changed after manifest creation".to_string(),
        ));
    }

    let memory_by_id: BTreeMap<_, _> = all_memories
        .into_iter()
        .map(|state| (state.id.clone(), state))
        .collect();
    if invalid_auto_links_after(&manifest.touching_links, &manifest.targets, &memory_by_id)
        != manifest.removed_links
    {
        return Err(ClioError::Conflict(
            "planned automatic-link removals do not match repaired endpoint state".to_string(),
        ));
    }

    for target in &manifest.targets {
        if load_memory_state(conn, &target.memory_id)?.as_ref() != Some(&target.before) {
            return Err(conflict("memory", &target.memory_id));
        }
        let attention = load_attention_for_memory(conn, &target.memory_id)?;
        let expected = target.attention.as_ref().map(|change| &change.before);
        if attention.as_ref() != expected {
            return Err(conflict("attention", &target.memory_id));
        }
    }
    let ids: BTreeSet<_> = manifest
        .targets
        .iter()
        .map(|target| target.memory_id.as_str())
        .collect();
    if load_touching_links(conn, &ids)? != manifest.touching_links {
        return Err(ClioError::Conflict(
            "complete touching-link set changed after manifest creation".to_string(),
        ));
    }
    Ok(())
}

fn invalid_auto_links_after(
    touching_links: &[LinkState],
    targets: &[RepairTarget],
    memory_by_id: &BTreeMap<String, MemoryState>,
) -> Vec<LinkState> {
    let after_by_id: BTreeMap<_, _> = targets
        .iter()
        .map(|target| (target.memory_id.as_str(), &target.after))
        .collect();
    touching_links
        .iter()
        .filter(|link| {
            if link.relationship != "auto:relates_to" {
                return false;
            }
            let from = after_by_id
                .get(link.from_memory_id.as_str())
                .copied()
                .or_else(|| memory_by_id.get(&link.from_memory_id));
            let to = after_by_id
                .get(link.to_memory_id.as_str())
                .copied()
                .or_else(|| memory_by_id.get(&link.to_memory_id));
            match (from, to) {
                (Some(from), Some(to)) => {
                    from.archived_at.is_some()
                        || to.archived_at.is_some()
                        || from.namespace != to.namespace
                }
                _ => true,
            }
        })
        .cloned()
        .collect()
}

fn verify_current_after_state(conn: &Connection, manifest: &RepairManifest) -> Result<()> {
    for target in &manifest.targets {
        if load_memory_state(conn, &target.memory_id)?.as_ref() != Some(&target.after) {
            return Err(conflict("memory", &target.memory_id));
        }
        let attention = load_attention_for_memory(conn, &target.memory_id)?;
        let expected = target.attention.as_ref().map(|change| &change.after);
        if attention.as_ref() != expected {
            return Err(conflict("attention", &target.memory_id));
        }
    }
    let ids: BTreeSet<_> = manifest
        .targets
        .iter()
        .map(|target| target.memory_id.as_str())
        .collect();
    let removed: BTreeSet<_> = manifest.removed_links.iter().collect();
    let expected: Vec<_> = manifest
        .touching_links
        .iter()
        .filter(|link| !removed.contains(link))
        .cloned()
        .collect();
    if load_touching_links(conn, &ids)? != expected {
        return Err(ClioError::Conflict(
            "complete touching-link set changed after repair".to_string(),
        ));
    }
    Ok(())
}

fn verify_current_rolled_back_state(conn: &Connection, manifest: &RepairManifest) -> Result<()> {
    for target in &manifest.targets {
        if load_memory_state(conn, &target.memory_id)?.as_ref() != Some(&target.before) {
            return Err(conflict("memory", &target.memory_id));
        }
        let attention = load_attention_for_memory(conn, &target.memory_id)?;
        let expected = target.attention.as_ref().map(|change| &change.before);
        if attention.as_ref() != expected {
            return Err(conflict("attention", &target.memory_id));
        }
    }
    let ids: BTreeSet<_> = manifest
        .targets
        .iter()
        .map(|target| target.memory_id.as_str())
        .collect();
    if load_touching_links(conn, &ids)? != manifest.touching_links {
        return Err(ClioError::Conflict(
            "touching-link set changed after repair rollback".to_string(),
        ));
    }
    Ok(())
}

struct JournalEntryInput {
    entity_type: String,
    entity_key: String,
    operation: String,
    before_json: Option<String>,
    after_json: Option<String>,
    evidence_json: String,
}

fn journal_entry<B: Serialize, A: Serialize>(
    entity_type: &str,
    entity_key: &str,
    operation: &str,
    before: Option<&B>,
    after: Option<&A>,
    evidence: &serde_json::Value,
) -> Result<JournalEntryInput> {
    Ok(JournalEntryInput {
        entity_type: entity_type.to_string(),
        entity_key: entity_key.to_string(),
        operation: operation.to_string(),
        before_json: before.map(serde_json::to_string).transpose()?,
        after_json: after.map(serde_json::to_string).transpose()?,
        evidence_json: serde_json::to_string(evidence)?,
    })
}

fn insert_journal_entry(
    conn: &Connection,
    transaction_id: &str,
    sequence: i64,
    entry: JournalEntryInput,
) -> Result<()> {
    conn.execute(
        "INSERT INTO repair_journal_entries
         (transaction_id, sequence, entity_type, entity_key, operation,
          before_json, after_json, evidence_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            transaction_id,
            sequence,
            entry.entity_type,
            entry.entity_key,
            entry.operation,
            entry.before_json,
            entry.after_json,
            entry.evidence_json,
        ],
    )?;
    Ok(())
}

fn journal_entry_count(conn: &Connection, transaction_id: &str) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM repair_journal_entries WHERE transaction_id = ?1",
        [transaction_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

fn link_key(link: &LinkState) -> String {
    format!(
        "{}|{}|{}",
        link.from_memory_id, link.to_memory_id, link.relationship
    )
}

fn conflict(entity_type: &str, key: &str) -> ClioError {
    ClioError::Conflict(format!(
        "{entity_type} {key} changed since the manifest snapshot"
    ))
}

fn finish_or_rollback<T>(conn: &Connection, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            finish_transaction(conn, true, "")?;
            Ok(value)
        }
        Err(error) => {
            rollback_db_transaction(conn, true, "");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LinkInput, RememberInput};
    use crate::{db, repository};

    fn remember(conn: &Connection, namespace: &str, source_ref: &str) -> String {
        repository::remember(
            conn,
            &RememberInput {
                namespace: namespace.to_string(),
                kind: "fact".to_string(),
                title: Some(source_ref.to_string()),
                summary: None,
                content: "stable content".to_string(),
                tags: Vec::new(),
                source: Some("repair-unit-test".to_string()),
                source_ref: Some(source_ref.to_string()),
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Default::default(),
        )
        .unwrap()
        .id
    }

    #[test]
    fn apply_rejects_a_resealed_manifest_that_removes_a_still_valid_auto_link() {
        let conn = db::open_in_memory().unwrap();
        let target = remember(&conn, "project:legacy", "target");
        let neighbour = remember(&conn, "project:canonical", "neighbour");
        repository::link(
            &conn,
            &LinkInput {
                from_memory_id: target.clone(),
                to_memory_id: neighbour,
                relationship: "auto:relates_to".to_string(),
                metadata: serde_json::json!({}),
            },
        )
        .unwrap();
        let mut manifest = build_manifest_at(
            &conn,
            &RepairPlan {
                intents: vec![RepairIntent {
                    memory_id: target,
                    action: RepairAction::Move {
                        namespace: "project:canonical".to_string(),
                    },
                    evidence: "test evidence".to_string(),
                }],
            },
            "2026-08-29T06:00:00Z",
        )
        .unwrap();
        assert!(manifest.removed_links.is_empty());

        manifest
            .removed_links
            .push(manifest.touching_links[0].clone());
        manifest.digest = manifest_payload_digest(&manifest).unwrap();
        manifest.transaction_id = format!("repair:{}", manifest.digest);

        let error = apply_manifest(&conn, &manifest).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("planned automatic-link removals")
        );
        assert_eq!(
            repository::get_links(&conn, &manifest.targets[0].memory_id)
                .unwrap()
                .len(),
            1
        );
    }
}
