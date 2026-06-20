import { invoke } from "@tauri-apps/api/core";
import type {
  BackupListEntry,
  BackupResult,
  BulkResult,
  CleanupCandidate,
  CleanupReport,
  DetectedContext,
  DuplicateScanResult,
  ImportResult,
  IntegrityReport,
  Memory,
  MemoryLink,
  MemoryStats,
  MergePreview,
  NamespaceInfo,
  RecallResult,
  RecentEntry,
  RememberInput,
  RestoreResult,
  SuggestionResult,
} from "./types";

export async function remember(input: RememberInput): Promise<Memory> {
  return invoke<Memory>("cmd_remember", { ...input });
}

export async function updateMemory(
  memoryId: string,
  input: RememberInput,
): Promise<Memory> {
  return invoke<Memory>("cmd_update", { memoryId, ...input });
}

export async function recall(params: {
  query?: string;
  namespace?: string;
  kind?: string;
  tags?: string[];
  match_all_tags?: boolean;
  importance_min?: number;
  importance_max?: number;
  sort_by?: string;
  include_archived?: boolean;
  limit?: number;
  offset?: number;
}): Promise<RecallResult> {
  return invoke<RecallResult>("cmd_recall", {
    query: params.query,
    namespace: params.namespace,
    kind: params.kind,
    tags: params.tags,
    matchAllTags: params.match_all_tags,
    importanceMin: params.importance_min,
    importanceMax: params.importance_max,
    sortBy: params.sort_by,
    includeArchived: params.include_archived,
    limit: params.limit,
    offset: params.offset,
  });
}

export async function getMemory(memoryId: string): Promise<Memory> {
  return invoke<Memory>("cmd_get", { memoryId });
}

export async function recent(params?: {
  namespace?: string;
  kind?: string;
  tags?: string[];
  match_all_tags?: boolean;
  importance_min?: number;
  importance_max?: number;
  sort_by?: string;
  include_archived?: boolean;
  limit?: number;
}): Promise<RecallResult> {
  // Tauri converts Rust snake_case params to camelCase on the JS side.
  const p = params ?? {};
  return invoke<RecallResult>("cmd_recent", {
    namespace: p.namespace,
    kind: p.kind,
    tags: p.tags,
    matchAllTags: p.match_all_tags,
    importanceMin: p.importance_min,
    importanceMax: p.importance_max,
    sortBy: p.sort_by,
    includeArchived: p.include_archived,
    limit: p.limit,
  });
}

export async function archive(memoryId: string): Promise<Memory> {
  return invoke<Memory>("cmd_archive", { memoryId });
}

export async function unarchive(memoryId: string): Promise<Memory> {
  return invoke<Memory>("cmd_unarchive", { memoryId });
}

export async function deleteMemory(memoryId: string): Promise<Memory> {
  return invoke<Memory>("cmd_delete", { memoryId });
}

export async function search(params: {
  query: string;
  namespace?: string;
  include_archived?: boolean;
  limit?: number;
}): Promise<RecallResult> {
  return invoke<RecallResult>("cmd_search", {
    query: params.query,
    namespace: params.namespace,
    includeArchived: params.include_archived,
    limit: params.limit,
  });
}

export async function link(params: {
  from_memory_id: string;
  to_memory_id: string;
  relationship?: string;
  metadata?: Record<string, unknown>;
}): Promise<MemoryLink> {
  return invoke<MemoryLink>("cmd_link", {
    fromMemoryId: params.from_memory_id,
    toMemoryId: params.to_memory_id,
    relationship: params.relationship,
    metadata: params.metadata,
  });
}

export async function getLinks(memoryId: string): Promise<MemoryLink[]> {
  return invoke<MemoryLink[]>("cmd_get_links", { memoryId });
}

export async function stats(namespace?: string): Promise<MemoryStats> {
  return invoke<MemoryStats>("cmd_stats", { namespace });
}

export async function activity(params?: {
  namespace?: string;
  limit?: number;
}): Promise<RecentEntry[]> {
  return invoke<RecentEntry[]>("cmd_activity", params ?? {});
}

export async function namespaces(): Promise<string[]> {
  return invoke<string[]>("cmd_namespaces");
}

export async function suggestLinks(params: {
  memory_id: string;
  threshold?: number;
  limit?: number;
}): Promise<SuggestionResult[]> {
  return invoke<SuggestionResult[]>("cmd_suggest_links", {
    memoryId: params.memory_id,
    threshold: params.threshold,
    limit: params.limit,
  });
}

export async function capture(params: {
  text: string;
  namespace?: string;
}): Promise<Memory> {
  return invoke<Memory>("cmd_capture", params);
}

export async function initNamespace(
  directory: string,
  namespace: string,
): Promise<void> {
  return invoke<void>("cmd_init_namespace", { directory, namespace });
}

export async function detectNamespace(
  directory: string,
): Promise<DetectedContext | null> {
  return invoke<DetectedContext | null>("cmd_detect_namespace", { directory });
}

// Bulk operations

export async function bulkArchive(memoryIds: string[]): Promise<BulkResult> {
  return invoke<BulkResult>("cmd_bulk_archive", { memoryIds });
}

export async function bulkDelete(memoryIds: string[]): Promise<BulkResult> {
  return invoke<BulkResult>("cmd_bulk_delete", { memoryIds });
}

export async function bulkAddTag(
  memoryIds: string[],
  tag: string,
): Promise<BulkResult> {
  return invoke<BulkResult>("cmd_bulk_add_tag", { memoryIds, tag });
}

export async function bulkRemoveTag(
  memoryIds: string[],
  tag: string,
): Promise<BulkResult> {
  return invoke<BulkResult>("cmd_bulk_remove_tag", { memoryIds, tag });
}

// Export / Import

export async function exportMemories(params?: {
  namespace?: string;
  include_archived?: boolean;
  format?: string;
}): Promise<string> {
  return invoke<string>("cmd_export_memories", {
    namespace: params?.namespace,
    includeArchived: params?.include_archived,
    format: params?.format,
  });
}

export async function importMemories(data: string): Promise<ImportResult> {
  return invoke<ImportResult>("cmd_import_memories", { data });
}

// Namespace management

export async function namespaceDetails(): Promise<NamespaceInfo[]> {
  return invoke<NamespaceInfo[]>("cmd_namespace_details");
}

export async function renameNamespace(
  from: string,
  to: string,
): Promise<number> {
  return invoke<number>("cmd_rename_namespace", { from, to });
}

export async function mergeNamespaces(
  source: string,
  target: string,
): Promise<number> {
  return invoke<number>("cmd_merge_namespaces", { source, target });
}

export async function deleteNamespace(namespace: string): Promise<boolean> {
  return invoke<boolean>("cmd_delete_namespace", { namespace });
}

export async function purgeNamespace(namespace: string): Promise<number> {
  return invoke<number>("cmd_purge_namespace", { namespace });
}

export interface CleanupCriteriaInput {
  staleMonths?: number | null;
  archived?: boolean;
  folderGone?: boolean;
  all?: boolean;
}

export async function findCleanupCandidates(
  criteria: CleanupCriteriaInput = {},
): Promise<CleanupCandidate[]> {
  return invoke<CleanupCandidate[]>("cmd_find_cleanup_candidates", {
    staleMonths: criteria.staleMonths ?? null,
    archived: criteria.archived ?? false,
    folderGone: criteria.folderGone ?? false,
    all: criteria.all ?? false,
  });
}

export async function runCleanup(namespaces: string[]): Promise<CleanupReport> {
  return invoke<CleanupReport>("cmd_run_cleanup", { namespaces });
}

// Integrity checks

export async function integrityCheck(): Promise<IntegrityReport> {
  return invoke<IntegrityReport>("cmd_integrity_check");
}

export async function integrityFix(): Promise<IntegrityReport> {
  return invoke<IntegrityReport>("cmd_integrity_fix");
}

// Backup and restore

export async function createBackup(): Promise<BackupResult> {
  return invoke<BackupResult>("cmd_backup");
}

export async function listBackups(): Promise<BackupListEntry[]> {
  return invoke<BackupListEntry[]>("cmd_list_backups");
}

export async function restoreBackup(
  backupPath: string,
): Promise<RestoreResult> {
  return invoke<RestoreResult>("cmd_restore", { backupPath });
}

// Deduplication

export async function findDuplicates(): Promise<DuplicateScanResult> {
  return invoke<DuplicateScanResult>("cmd_find_duplicates");
}

export async function previewMerge(
  keepId: string,
  mergeIds: string[],
): Promise<MergePreview> {
  return invoke<MergePreview>("cmd_preview_merge", { keepId, mergeIds });
}

export async function mergeMemories(
  keepId: string,
  mergeIds: string[],
): Promise<Memory> {
  return invoke<Memory>("cmd_merge_memories", { keepId, mergeIds });
}
