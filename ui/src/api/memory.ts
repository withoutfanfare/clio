import { invoke } from "@tauri-apps/api/core";
import type {
  DetectedContext,
  Memory,
  MemoryLink,
  MemoryStats,
  RecallResult,
  RecentEntry,
  RememberInput,
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
