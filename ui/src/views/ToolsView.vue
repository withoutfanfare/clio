<script setup lang="ts">
import { ref } from "vue";
import * as api from "@/api/memory";
import { useMemoryStore } from "@/stores/memories";
import type { IntegrityReport, BackupListEntry, ImportResult } from "@/api/types";

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

// Load backups on mount
loadBackups();
</script>

<template>
  <div class="tools-view">
    <h1 class="tools-title">Tools</h1>

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
