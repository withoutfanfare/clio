<script setup lang="ts">
import { ref } from "vue";
import * as api from "@/api/memory";
import { useMemoryStore } from "@/stores/memories";
import type {
  IntegrityReport,
  BackupListEntry,
  ImportResult,
  DuplicateScanResult,
  DuplicateCluster,
  MergePreview,
} from "@/api/types";

const store = useMemoryStore();

// Integrity
const integrityReport = ref<IntegrityReport | null>(null);
const integrityLoading = ref(false);
const integrityError = ref<string | null>(null);

async function runIntegrityCheck() {
  integrityLoading.value = true;
  integrityError.value = null;
  try {
    integrityReport.value = await api.integrityCheck();
  } catch (e) {
    integrityError.value = String(e);
  } finally {
    integrityLoading.value = false;
  }
}

async function fixIntegrityIssues() {
  integrityLoading.value = true;
  integrityError.value = null;
  try {
    integrityReport.value = await api.integrityFix();
    store.invalidateSearchCache();
  } catch (e) {
    integrityError.value = String(e);
  } finally {
    integrityLoading.value = false;
  }
}

// Backup
const backups = ref<BackupListEntry[]>([]);
const backupLoading = ref(false);
const backupMessage = ref<string | null>(null);
const backupError = ref<string | null>(null);
const confirmingRestore = ref<string | null>(null);

async function createBackup() {
  backupLoading.value = true;
  backupError.value = null;
  try {
    const result = await api.createBackup();
    backupMessage.value = `Backup created: ${result.path}`;
    await loadBackups();
  } catch (e) {
    backupError.value = String(e);
  } finally {
    backupLoading.value = false;
  }
}

async function loadBackups() {
  try {
    backups.value = await api.listBackups();
  } catch (e) {
    backupError.value = String(e);
  }
}

async function restoreBackup(path: string) {
  if (confirmingRestore.value !== path) {
    confirmingRestore.value = path;
    return;
  }
  backupLoading.value = true;
  backupError.value = null;
  try {
    await api.restoreBackup(path);
    backupMessage.value = "Database restored successfully. Please restart the app.";
    confirmingRestore.value = null;
    store.invalidateSearchCache();
  } catch (e) {
    backupError.value = String(e);
    confirmingRestore.value = null;
  } finally {
    backupLoading.value = false;
  }
}

// Export
const exportLoading = ref(false);
const exportMessage = ref<string | null>(null);
const exportError = ref<string | null>(null);

async function exportAll() {
  exportLoading.value = true;
  exportError.value = null;
  try {
    const data = await api.exportMemories({ include_archived: true, format: "json" });
    const blob = new Blob([data], { type: "application/jsonl;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    a.download = `clio-export-${ts}.jsonl`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    const lines = data.split("\n").filter((l: string) => l.trim()).length;
    exportMessage.value = `Exported ${lines} memories`;
  } catch (e) {
    exportError.value = String(e);
  } finally {
    exportLoading.value = false;
  }
}

// Import
const importLoading = ref(false);
const importResult = ref<ImportResult | null>(null);
const importError = ref<string | null>(null);
const fileInputRef = ref<HTMLInputElement | null>(null);

function triggerImport() {
  fileInputRef.value?.click();
}

async function handleImportFile(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;

  importLoading.value = true;
  importError.value = null;
  importResult.value = null;

  try {
    const text = await file.text();
    importResult.value = await api.importMemories(text);
    store.invalidateSearchCache();
    await store.loadRecent();
    await store.fetchNamespaces();
  } catch (e) {
    importError.value = String(e);
  } finally {
    importLoading.value = false;
    input.value = "";
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

function formatDate(iso: string): string {
  if (!iso || iso === "unknown") return "—";
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

// Deduplication
const dedupResult = ref<DuplicateScanResult | null>(null);
const dedupLoading = ref(false);
const dedupError = ref<string | null>(null);
const mergePreview = ref<MergePreview | null>(null);
const mergePreviewCluster = ref<DuplicateCluster | null>(null);
const mergeLoading = ref(false);
const mergeMessage = ref<string | null>(null);

async function runDeduplicationScan() {
  dedupLoading.value = true;
  dedupError.value = null;
  mergePreview.value = null;
  mergePreviewCluster.value = null;
  mergeMessage.value = null;
  try {
    dedupResult.value = await api.findDuplicates();
  } catch (e) {
    dedupError.value = String(e);
  } finally {
    dedupLoading.value = false;
  }
}

async function showMergePreview(cluster: DuplicateCluster) {
  if (cluster.memories.length < 2) return;
  const keepId = cluster.memories[0].id;
  const mergeIds = cluster.memories.slice(1).map((m) => m.id);
  mergePreviewCluster.value = cluster;
  mergeLoading.value = true;
  try {
    mergePreview.value = await api.previewMerge(keepId, mergeIds);
  } catch (e) {
    dedupError.value = String(e);
  } finally {
    mergeLoading.value = false;
  }
}

async function executeMerge() {
  if (!mergePreview.value || !mergePreviewCluster.value) return;
  const keepId = mergePreview.value.keep_id;
  const mergeIds = mergePreviewCluster.value.memories
    .slice(1)
    .map((m) => m.id);
  mergeLoading.value = true;
  try {
    await api.mergeMemories(keepId, mergeIds);
    mergeMessage.value = `Merged ${mergeIds.length} memories into one`;
    mergePreview.value = null;
    mergePreviewCluster.value = null;
    store.invalidateSearchCache();
    // Re-scan to update results.
    await runDeduplicationScan();
    await store.loadRecent();
  } catch (e) {
    dedupError.value = String(e);
  } finally {
    mergeLoading.value = false;
  }
}

function dismissMergePreview() {
  mergePreview.value = null;
  mergePreviewCluster.value = null;
}

function truncateContent(content: string, maxLen = 120): string {
  if (content.length <= maxLen) return content;
  return content.slice(0, maxLen) + "\u2026";
}

function similarityLabel(sim: number): string {
  if (sim >= 1.0) return "Exact match";
  if (sim >= 0.8) return `${Math.round(sim * 100)}% similar`;
  return `${Math.round(sim * 100)}% similar`;
}

// Load backups on mount
loadBackups();
</script>

<template>
  <div class="tools-view">
    <h1 class="tools-title">Tools</h1>

    <!-- Deduplication Section -->
    <section class="tools-section">
      <h2 class="section-title">Deduplication</h2>
      <p class="section-desc">
        Scan for duplicate and near-duplicate memories. Review clusters and merge them to keep the database clean.
      </p>
      <div class="section-actions">
        <button class="tool-btn" @click="runDeduplicationScan" :disabled="dedupLoading">
          {{ dedupLoading ? "Scanning\u2026" : "Scan for duplicates" }}
        </button>
      </div>

      <div v-if="dedupError" class="tool-error" @click="dedupError = null">{{ dedupError }}</div>
      <div v-if="mergeMessage" class="tool-success" @click="mergeMessage = null">{{ mergeMessage }}</div>

      <div v-if="dedupResult" class="dedup-report">
        <div class="report-summary">
          <span>Scanned {{ dedupResult.total_scanned }} memories</span>
          <span v-if="dedupResult.clusters.length === 0" class="report-ok">No duplicates found</span>
          <span v-else class="report-issues">
            {{ dedupResult.clusters.length }} cluster{{ dedupResult.clusters.length === 1 ? "" : "s" }} found
          </span>
        </div>

        <div v-for="(cluster, ci) in dedupResult.clusters" :key="ci" class="dedup-cluster">
          <div class="cluster-header">
            <span class="cluster-badge" :class="cluster.match_type">
              {{ cluster.match_type === 'exact' ? 'Exact' : similarityLabel(cluster.similarity) }}
            </span>
            <span class="cluster-count">{{ cluster.memories.length }} memories</span>
            <button
              class="tool-btn-sm"
              @click="showMergePreview(cluster)"
              :disabled="mergeLoading"
            >
              Preview merge
            </button>
          </div>

          <div v-for="mem in cluster.memories" :key="mem.id" class="cluster-memory">
            <div class="mem-title">{{ mem.title || truncateContent(mem.content, 80) }}</div>
            <div class="mem-meta">
              <span class="mem-ns">{{ mem.namespace }}</span>
              <span class="mem-kind">{{ mem.kind }}</span>
              <span v-if="mem.tags.length" class="mem-tags">{{ mem.tags.join(', ') }}</span>
              <span class="mem-date">{{ formatDate(mem.updated_at) }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Merge Preview Modal -->
      <div v-if="mergePreview" class="merge-preview">
        <h3 class="merge-title">Merge Preview</h3>
        <p class="section-desc">
          The first memory will be kept. {{ mergePreview.memories_archived }} other{{ mergePreview.memories_archived === 1 ? '' : 's' }} will be archived.
        </p>
        <div class="merge-detail">
          <div class="merge-field">
            <span class="merge-label">Title</span>
            <span class="merge-value">{{ mergePreview.title || '(none)' }}</span>
          </div>
          <div class="merge-field">
            <span class="merge-label">Content</span>
            <span class="merge-value merge-content">{{ truncateContent(mergePreview.content, 200) }}</span>
          </div>
          <div class="merge-field">
            <span class="merge-label">Tags</span>
            <span class="merge-value">{{ mergePreview.tags.length ? mergePreview.tags.join(', ') : '(none)' }}</span>
          </div>
          <div class="merge-field">
            <span class="merge-label">Importance</span>
            <span class="merge-value">{{ mergePreview.importance }}</span>
          </div>
          <div v-if="mergePreview.confidence !== null" class="merge-field">
            <span class="merge-label">Confidence</span>
            <span class="merge-value">{{ mergePreview.confidence }}</span>
          </div>
          <div v-if="mergePreview.links_transferred > 0" class="merge-field">
            <span class="merge-label">Links transferred</span>
            <span class="merge-value">{{ mergePreview.links_transferred }}</span>
          </div>
        </div>
        <div class="section-actions">
          <button class="tool-btn primary" @click="executeMerge" :disabled="mergeLoading">
            {{ mergeLoading ? "Merging\u2026" : "Confirm merge" }}
          </button>
          <button class="tool-btn" @click="dismissMergePreview">Cancel</button>
        </div>
      </div>
    </section>

    <!-- Integrity Section -->
    <section class="tools-section">
      <h2 class="section-title">Integrity Check</h2>
      <p class="section-desc">
        Scan the memory database for broken links, orphaned entries, duplicates, and tag mismatches.
      </p>
      <div class="section-actions">
        <button class="tool-btn" @click="runIntegrityCheck" :disabled="integrityLoading">
          {{ integrityLoading ? "Checking\u2026" : "Run check" }}
        </button>
        <button
          v-if="integrityReport && integrityReport.issues.some((i) => i.auto_fixable)"
          class="tool-btn primary"
          @click="fixIntegrityIssues"
          :disabled="integrityLoading"
        >
          Auto-fix issues
        </button>
      </div>

      <div v-if="integrityError" class="tool-error">{{ integrityError }}</div>

      <div v-if="integrityReport" class="integrity-report">
        <div class="report-summary">
          <span>Checked {{ integrityReport.total_checked }} memories</span>
          <span v-if="integrityReport.issues_found === 0" class="report-ok">No issues found</span>
          <span v-else class="report-issues">{{ integrityReport.issues_found }} issues found</span>
          <span v-if="integrityReport.fixed > 0" class="report-fixed">{{ integrityReport.fixed }} fixed</span>
        </div>
        <div v-for="issue in integrityReport.issues" :key="issue.kind" class="issue-card">
          <div class="issue-header">
            <span class="issue-kind">{{ issue.kind.replace(/_/g, " ") }}</span>
            <span v-if="issue.auto_fixable" class="issue-fixable">Auto-fixable</span>
          </div>
          <p class="issue-desc">{{ issue.description }}</p>
          <p class="issue-fix">Suggested: {{ issue.suggested_fix }}</p>
        </div>
      </div>
    </section>

    <!-- Backup Section -->
    <section class="tools-section">
      <h2 class="section-title">Database Backup</h2>
      <p class="section-desc">
        Create timestamped backups of the SQLite database. Keeps the last 5 backups.
      </p>
      <div class="section-actions">
        <button class="tool-btn primary" @click="createBackup" :disabled="backupLoading">
          {{ backupLoading ? "Backing up\u2026" : "Create backup" }}
        </button>
      </div>

      <div v-if="backupMessage" class="tool-success" @click="backupMessage = null">{{ backupMessage }}</div>
      <div v-if="backupError" class="tool-error" @click="backupError = null">{{ backupError }}</div>

      <div v-if="backups.length" class="backup-list">
        <div v-for="backup in backups" :key="backup.path" class="backup-item">
          <div class="backup-info">
            <span class="backup-name">{{ backup.filename }}</span>
            <span class="backup-meta">{{ formatSize(backup.size_bytes) }} &middot; {{ formatDate(backup.created) }}</span>
          </div>
          <button
            class="tool-btn-sm"
            :class="{ danger: confirmingRestore === backup.path }"
            @click="restoreBackup(backup.path)"
          >
            {{ confirmingRestore === backup.path ? "Confirm restore" : "Restore" }}
          </button>
        </div>
      </div>
    </section>

    <!-- Export / Import Section -->
    <section class="tools-section">
      <h2 class="section-title">Export &amp; Import</h2>
      <p class="section-desc">
        Export all memories to JSONL or import from a backup file.
      </p>
      <div class="section-actions">
        <button class="tool-btn" @click="exportAll" :disabled="exportLoading">
          {{ exportLoading ? "Exporting\u2026" : "Export all (JSONL)" }}
        </button>
        <button class="tool-btn" @click="triggerImport" :disabled="importLoading">
          {{ importLoading ? "Importing\u2026" : "Import from JSONL" }}
        </button>
        <input
          ref="fileInputRef"
          type="file"
          accept=".jsonl,.json,.txt"
          class="hidden-input"
          @change="handleImportFile"
        />
      </div>

      <div v-if="exportMessage" class="tool-success" @click="exportMessage = null">{{ exportMessage }}</div>
      <div v-if="exportError" class="tool-error" @click="exportError = null">{{ exportError }}</div>

      <div v-if="importResult" class="import-report">
        <span class="import-stat import-ok">{{ importResult.imported }} imported</span>
        <span v-if="importResult.skipped > 0" class="import-stat import-skip">{{ importResult.skipped }} skipped</span>
        <div v-if="importResult.errors.length" class="import-errors">
          <p v-for="(err, i) in importResult.errors.slice(0, 5)" :key="i" class="import-error-line">{{ err }}</p>
        </div>
      </div>
      <div v-if="importError" class="tool-error" @click="importError = null">{{ importError }}</div>
    </section>
  </div>
</template>

<style scoped>
.tools-view {
  padding-bottom: var(--space-12);
}

.tools-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  letter-spacing: var(--tracking-tight);
  color: var(--colour-text);
  margin-bottom: var(--space-6);
}

.tools-section {
  margin-bottom: var(--space-8);
  padding: var(--space-5);
  background: var(--colour-surface-card);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
}

.section-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
  margin-bottom: var(--space-2);
}

.section-desc {
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  margin-bottom: var(--space-4);
  line-height: var(--leading-relaxed);
}

.section-actions {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.tool-btn {
  padding: var(--space-2) var(--space-4);
  background: var(--colour-surface-overlay);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: all 150ms;
}

.tool-btn:hover:not(:disabled) {
  border-color: var(--colour-border-hover);
  background: var(--colour-surface-input);
}

.tool-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.tool-btn.primary {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
  color: white;
}

.tool-btn.primary:hover:not(:disabled) {
  background: var(--colour-accent-hover);
}

.tool-btn-sm {
  padding: var(--space-1) var(--space-3);
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: all 150ms;
  white-space: nowrap;
}

.tool-btn-sm:hover {
  color: var(--colour-text);
  border-color: var(--colour-border-hover);
}

.tool-btn-sm.danger {
  color: var(--colour-danger);
  border-color: var(--colour-danger);
  font-weight: var(--font-medium);
}

.tool-success {
  padding: var(--space-2) var(--space-3);
  background: color-mix(in srgb, var(--colour-success) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-success) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-success);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
  cursor: pointer;
}

.tool-error {
  padding: var(--space-2) var(--space-3);
  background: color-mix(in srgb, var(--colour-danger) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-danger) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-danger);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
  cursor: pointer;
}

/* Deduplication */
.dedup-report {
  margin-top: var(--space-3);
}

.dedup-cluster {
  padding: var(--space-3);
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
  margin-bottom: var(--space-2);
}

.cluster-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.cluster-badge {
  font-size: var(--text-xs);
  padding: 1px 6px;
  border-radius: 99px;
  font-weight: var(--font-medium);
}

.cluster-badge.exact {
  background: color-mix(in srgb, var(--colour-danger) 15%, transparent);
  color: var(--colour-danger);
}

.cluster-badge.similar {
  background: color-mix(in srgb, var(--colour-warning) 15%, transparent);
  color: var(--colour-warning);
}

.cluster-count {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
  flex: 1;
}

.cluster-memory {
  padding: var(--space-2);
  border-top: 1px solid var(--glass-border);
}

.cluster-memory:first-of-type {
  border-top: none;
}

.mem-title {
  font-size: var(--text-sm);
  color: var(--colour-text);
  margin-bottom: 2px;
}

.mem-meta {
  display: flex;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

.mem-ns {
  color: var(--colour-accent);
}

.mem-kind {
  text-transform: capitalize;
}

.mem-tags {
  opacity: 0.7;
}

/* Merge Preview */
.merge-preview {
  margin-top: var(--space-4);
  padding: var(--space-4);
  background: var(--colour-surface-card);
  border: 1px solid var(--colour-accent);
  border-radius: var(--radius-lg);
}

.merge-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
  margin-bottom: var(--space-2);
}

.merge-detail {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-bottom: var(--space-4);
}

.merge-field {
  display: flex;
  gap: var(--space-3);
  font-size: var(--text-sm);
}

.merge-label {
  min-width: 100px;
  color: var(--colour-text-muted);
  font-weight: var(--font-medium);
}

.merge-value {
  color: var(--colour-text);
}

.merge-content {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  font-size: var(--text-xs);
  white-space: pre-wrap;
}

/* Integrity */
.integrity-report {
  margin-top: var(--space-3);
}

.report-summary {
  display: flex;
  gap: var(--space-3);
  font-size: var(--text-sm);
  color: var(--colour-text-secondary);
  margin-bottom: var(--space-3);
}

.report-ok { color: var(--colour-success); font-weight: var(--font-medium); }
.report-issues { color: var(--colour-warning); font-weight: var(--font-medium); }
.report-fixed { color: var(--colour-success); font-weight: var(--font-medium); }

.issue-card {
  padding: var(--space-3);
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
  margin-bottom: var(--space-2);
}

.issue-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-1);
}

.issue-kind {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--colour-warning);
  text-transform: capitalize;
}

.issue-fixable {
  font-size: var(--text-xs);
  padding: 1px 6px;
  border-radius: 99px;
  background: color-mix(in srgb, var(--colour-success) 15%, transparent);
  color: var(--colour-success);
}

.issue-desc {
  font-size: var(--text-sm);
  color: var(--colour-text-secondary);
  margin: 0;
}

.issue-fix {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
  margin: var(--space-1) 0 0;
}

/* Backup */
.backup-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.backup-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3);
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
}

.backup-info {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.backup-name {
  font-size: var(--text-sm);
  color: var(--colour-text);
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  font-size: var(--text-xs);
}

.backup-meta {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

/* Import */
.hidden-input {
  display: none;
}

.import-report {
  display: flex;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.import-stat {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.import-ok { color: var(--colour-success); }
.import-skip { color: var(--colour-warning); }

.import-errors {
  margin-top: var(--space-2);
}

.import-error-line {
  font-size: var(--text-xs);
  color: var(--colour-danger);
  margin: 0 0 2px;
}
</style>
