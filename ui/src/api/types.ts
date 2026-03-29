export interface Memory {
  id: string;
  namespace: string;
  kind: string;
  title: string | null;
  summary: string | null;
  content: string;
  tags: string[];
  source: string | null;
  source_ref: string | null;
  confidence: number | null;
  importance: number;
  metadata: Record<string, unknown>;
  valid_from: string | null;
  valid_until: string | null;
  archived_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface RecallItem {
  id: string;
  namespace: string;
  kind: string;
  title: string | null;
  summary: string | null;
  content: string;
  tags: string[];
  source: string | null;
  source_ref: string | null;
  confidence: number | null;
  importance: number;
  metadata: Record<string, unknown>;
  valid_from: string | null;
  valid_until: string | null;
  archived_at: string | null;
  created_at: string;
  updated_at: string;
  rank: number | null;
  linked_from: string | null;
}

export interface RecallResult {
  total: number;
  count: number;
  offset: number;
  limit: number;
  items: RecallItem[];
}

export interface MemoryLink {
  from_memory_id: string;
  to_memory_id: string;
  relationship: string;
  metadata: Record<string, unknown>;
  created_at: string;
}

export interface MemoryStats {
  total_memories: number;
  active_memories: number;
  archived_memories: number;
  total_embeddings: number;
  embedding_coverage: number;
  by_namespace: [string, number][];
  by_kind: [string, number][];
  by_week: [string, number][];
  top_tags: [string, number][];
  total_links: number;
  link_density: number;
}

export interface RecentEntry {
  memory_id: string;
  title: string | null;
  namespace: string;
  kind: string;
  action: "created" | "updated" | "archived";
  timestamp: string;
}

export interface RememberInput {
  namespace?: string;
  kind?: string;
  title?: string;
  summary?: string;
  content: string;
  tags?: string[];
  source?: string;
  source_ref?: string;
  confidence?: number;
  importance?: number;
  metadata?: Record<string, unknown>;
  upsert?: boolean;
}

export interface SuggestionResult {
  memory: Memory;
  similarity: number;
}

export interface DetectedContext {
  namespace: string;
  source: "clio_namespace_file" | "git_directory" | "project_manifest";
  marker_path: string;
}

// Bulk operations
export interface BulkResult {
  affected: number;
}

// Namespace management
export interface NamespaceInfo {
  name: string;
  memory_count: number;
  last_activity: string | null;
}

// Integrity checks
export interface IntegrityIssue {
  kind: string;
  description: string;
  suggested_fix: string;
  auto_fixable: boolean;
  affected_ids: string[];
}

export interface IntegrityReport {
  issues: IntegrityIssue[];
  total_checked: number;
  issues_found: number;
  fixed: number;
}

// Backup and restore
export interface BackupResult {
  path: string;
  size_bytes: number;
  timestamp: string;
}

export interface BackupListEntry {
  path: string;
  filename: string;
  size_bytes: number;
  created: string;
}

export interface RestoreResult {
  restored_from: string;
  integrity_ok: boolean;
}

// Import
export interface ImportResult {
  imported: number;
  skipped: number;
  errors: string[];
}

// Deduplication
export interface DuplicateCluster {
  memories: Memory[];
  similarity: number;
  match_type: "exact" | "similar";
}

export interface DuplicateScanResult {
  clusters: DuplicateCluster[];
  total_scanned: number;
  duplicates_found: number;
}

export interface MergePreview {
  keep_id: string;
  content: string;
  title: string | null;
  tags: string[];
  confidence: number | null;
  importance: number;
  namespace: string;
  kind: string;
  links_transferred: number;
  memories_archived: number;
}
