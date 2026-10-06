<script setup lang="ts">
import { computed, ref, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { NextActor, TaskState, TaskView, Recommendation, RunView } from "@/api/work";
import { useWorkOverview } from "@/composables/useWorkOverview";

const { overview, loading, error, checkedAt, refresh } = useWorkOverview();
const stateLabels: Record<TaskState, string> = {
  running: "In progress", needs_user: "Needs Danny", waiting: "Waiting",
  accepted: "Accepted", stopped: "Stopped", implemented: "Ready for review",
  reporting_missing: "Reporting missing", conflict: "Conflicting reports",
};
const actorLabels: Record<NextActor, string> = {
  agent: "Agent", user: "Danny", other: "Other", none: "No action required",
};

function activeRuns(task: TaskView) { return task.runs.filter(run => !run.superseded); }
function needsDanny(task: TaskView) { return reviewableRuns(task).length > 0 || activeRuns(task).some(run => !run.accepted && run.receipt.report.next_actor === "user"); }
function uncertain(task: TaskView) { return task.state === "conflict" || (task.stale && !["implemented", "accepted"].includes(task.state)) || task.source_unavailable; }
function taskSummary(task: TaskView): string {
  if (task.state === "accepted") return "The reported scope has human acceptance.";
  if (task.state === "conflict") return "Conflicting reports — open the task details before choosing the next action.";
  return latestPendingRun(task)?.receipt.report.summary || "No summary reported.";
}
function nextActionLabel(task: TaskView): string {
  if (task.state === "conflict") return "Next owner unresolved";
  if (task.state === "accepted") return "No action required";
  if (task.state === "implemented") return "Danny review";
  const labels: Record<NextActor, string> = { agent: "Agent next", user: "Danny next", other: "Other next", none: "No action required" };
  return labels[task.next_actor] || "Next owner unresolved";
}
function nextStep(task: TaskView): string {
  if (task.state === "accepted") return "No action needed for the accepted scope.";
  if (task.state === "implemented") return overview.value?.can_accept ? "Check the result, then accept this change here." : "Check the result. Acceptance is available in the local dashboard.";
  if (task.state === "conflict") return "Resolve the differing reports before choosing the next step.";
  return latestPendingRun(task)?.receipt.report.next_step || "No next step reported.";
}
function reviewableRuns(task: TaskView) {
  return task.state === "implemented" ? activeRuns(task).filter(run => !run.accepted && run.receipt.report.state === "implemented") : [];
}
const reviewCount = computed(() => (overview.value?.tasks ?? []).filter(task => reviewableRuns(task).length).length);
const runningCount = computed(() => (overview.value?.tasks ?? []).filter(task => task.state === "running").length);
const orientation = computed(() => {
  if (error.value) return "The current position needs checking";
  if (runningCount.value) return "Work is in progress";
  if (reviewCount.value) return "Changes are ready for your review";
  if (needsCount.value) return "Work is waiting for you";
  if (uncertainCount.value) return "Some work needs a status check";
  if (overview.value?.tasks.every(task => task.state === "accepted")) return "Reported changes are accepted";
  return "Work is paused";
});
const review = ref<{
  project: string; task: string; title: string; receiptId: number; summary: string;
  observedAt: number; evidence: string[]; openable: string[];
} | null>(null);
const savingAcceptance = ref(false);
const reviewError = ref("");
const reviewMessage = ref("");
const canConfirmReview = computed(() => !!review.value && !error.value && !loading.value &&
  !!overview.value?.can_accept && (overview.value.tasks ?? []).some(task =>
    task.project === review.value!.project && task.task === review.value!.task &&
    reviewableRuns(task).some(run => run.receipt.id === review.value!.receiptId)));
function startReview(task: TaskView, run: RunView) {
  if (savingAcceptance.value || !overview.value?.can_accept || !reviewableRuns(task).some(item => item.receipt.id === run.receipt.id)) return;
  const report = run.receipt.report;
  review.value = { project: task.project, task: task.task, title: report.task_title || task.task_title,
    receiptId: run.receipt.id, summary: report.summary, observedAt: report.observed_at,
    evidence: [...report.evidence], openable: [...(run.openable_evidence ?? [])] };
  reviewError.value = "";
  reviewMessage.value = "";
}
function cancelReview() {
  if (!savingAcceptance.value) { review.value = null; reviewError.value = ""; }
}
async function acceptReview() {
  if (!review.value || !canConfirmReview.value || savingAcceptance.value) return;
  savingAcceptance.value = true;
  reviewError.value = "";
  const reviewed = review.value;
  try {
    await invoke("cmd_accept_work_change", { receiptId: reviewed.receiptId });
    review.value = null;
    reviewMessage.value = `Acceptance saved for “${reviewed.title}”.`;
    await refresh();
  } catch {
    reviewError.value = "Could not save acceptance. Refresh and review the current report before trying again.";
  } finally {
    savingAcceptance.value = false;
  }
}
const copyStatus = ref<{ prompt: string; message: string } | null>(null);
async function copyPrompt(recommendation: Recommendation) {
  try {
    await invoke("cmd_copy_to_clipboard", { text: recommendation.prompt });
    copyStatus.value = { prompt: recommendation.prompt, message: "Copied — paste into a new session when ready." };
  } catch {
    copyStatus.value = { prompt: recommendation.prompt, message: "Could not copy. Select and copy the prompt below." };
  }
}
const openStatus = ref<{ receiptId: number; reference: string; message: string } | null>(null);
async function openEvidence(receiptId: number, reference: string) {
  try {
    await invoke("cmd_open_work_evidence", { receiptId, reference });
    openStatus.value = { receiptId, reference, message: "Opened in the default app." };
  } catch {
    openStatus.value = { receiptId, reference, message: "Could not open. The file may be missing, unsupported or outside this task’s worktree." };
  }
}
function timestamp(seconds: number) {
  return new Date(seconds * 1000).toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" });
}
const relativeTime = new Intl.RelativeTimeFormat("en-GB");
function reportAge(task: TaskView): string {
  const observed = Math.max(...activeRuns(task).map(run => run.receipt.report.observed_at).filter(Number.isFinite));
  if (!Number.isFinite(observed)) return "Report time unavailable";
  const age = Math.floor(Date.now() / 1000) - observed;
  if (age < 0) return "Report time is in the future";
  if (age < 60) return "Latest report just now";
  const [size, unit]: [number, Intl.RelativeTimeFormatUnit] = age < 3600 ? [60, "minute"] : age < 86400 ? [3600, "hour"] : [86400, "day"];
  return `Latest report ${relativeTime.format(-Math.floor(age / size), unit)}`;
}
function lastObserved(task: TaskView): number | null {
  const times = activeRuns(task).map(run => run.receipt.report.observed_at).filter(Number.isFinite);
  return times.length ? Math.max(...times) : null;
}
function latestPendingRun(task: TaskView) {
  return activeRuns(task).filter(run => !run.accepted).sort((a, b) =>
    (b.receipt.report.observed_at ?? -Infinity) - (a.receipt.report.observed_at ?? -Infinity))[0];
}
function taskKey(task: TaskView) { return JSON.stringify([task.project, task.task]); }
const orderedTasks = computed(() => [...(overview.value?.tasks ?? [])].sort((a, b) =>
  ((lastObserved(b) ?? -Infinity) - (lastObserved(a) ?? -Infinity)) || taskKey(a).localeCompare(taskKey(b))));
const projects = computed(() => [...new Set(orderedTasks.value.map(task => task.project))].sort());
const filter = ref<"all" | "needs-you" | "accepted">("all");
const projectFilter = ref("");
const selectedKey = ref<string | null>(null);
const mobileDetail = ref(false);
const detailHeading = ref<HTMLElement | null>(null);
const visibleTasks = computed(() => orderedTasks.value.filter(task =>
  (!projectFilter.value || task.project === projectFilter.value) &&
  (filter.value === "all" || (filter.value === "needs-you" ? needsDanny(task) : task.state === "accepted"))));
const selectedTask = computed(() => orderedTasks.value.find(task => taskKey(task) === selectedKey.value) ?? null);
watch([visibleTasks, review, savingAcceptance], ([tasks]) => {
  if (!review.value && !savingAcceptance.value && !tasks.some(task => taskKey(task) === selectedKey.value)) {
    selectedKey.value = tasks[0] ? taskKey(tasks[0]) : null;
    if (!selectedKey.value) mobileDetail.value = false;
  }
});
function selectTask(task: TaskView) {
  if (savingAcceptance.value) return;
  cancelReview();
  selectedKey.value = taskKey(task);
  mobileDetail.value = true;
  void nextTick(() => detailHeading.value?.focus({ preventScroll: true }));
}
function changeFilter(value: "all" | "needs-you" | "accepted") {
  if (savingAcceptance.value) return;
  cancelReview(); filter.value = value; mobileDetail.value = false;
}
function rowDate(task: TaskView) {
  const observed = lastObserved(task);
  return observed === null ? "Time unavailable" : new Date(observed * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}
function rowTime(task: TaskView) {
  const observed = lastObserved(task);
  return observed === null ? "" : new Date(observed * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
}
function observationISO(task: TaskView) {
  const observed = lastObserved(task);
  return observed === null ? undefined : new Date(observed * 1000).toISOString();
}
const needsCount = computed(() => (overview.value?.tasks ?? []).filter(needsDanny).length);
const uncertainCount = computed(() => (overview.value?.tasks ?? []).filter(uncertain).length);
</script>

<template>
  <div class="work-view" :data-detail="mobileDetail">
    <header class="work-header">
      <div><h1>Work</h1><p class="muted">Progress, decisions and what happens next</p></div>
      <div class="refresh-control">
        <span v-if="checkedAt" class="muted" :title="`Dashboard checked ${timestamp(checkedAt / 1000)}`">Checked {{ new Date(checkedAt).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }) }}</span>
        <button class="quiet-button" type="button" :disabled="loading" @click="refresh">{{ loading ? "Refreshing…" : "Refresh" }}</button>
      </div>
    </header>
    <div v-if="error" class="work-warning" role="alert">
      <strong>{{ overview ? "Refresh failed — showing the last received reports." : "Work reports could not be loaded." }}</strong>
      <p>Current position is unverified. Refresh to try again.</p>
      <details><summary>Error details</summary><p>{{ error }}</p></details>
    </div>
    <p v-if="!overview && loading" class="loading-state" role="status">Loading your work…</p>
    <p v-if="reviewMessage" role="status" class="review-message">{{ reviewMessage }}</p>
    <template v-if="overview">
      <div class="status-line" :aria-label="orientation">
        <span><i class="status-dot running"></i><strong>{{ runningCount }}</strong> in progress</span>
        <span><i class="status-dot implemented"></i><strong>{{ reviewCount }}</strong> ready for review</span>
        <span v-if="uncertainCount" class="warning-text">{{ uncertainCount }} need a status check</span>
      </div>
      <div v-if="!overview.tasks.length" class="empty-state"><h2>No reports yet</h2><p>Work appears here when an agent reports. An empty view does not mean nothing is happening.</p></div>
      <div v-else class="work-layout">
        <section class="task-browser" aria-label="Work overview">
          <div class="browser-toolbar">
            <div class="view-filters" aria-label="Filter work">
              <button type="button" :aria-pressed="filter === 'all'" :disabled="savingAcceptance" @click="changeFilter('all')">All updates</button>
              <button type="button" :aria-pressed="filter === 'needs-you'" :disabled="savingAcceptance" @click="changeFilter('needs-you')">Needs you <span>{{ needsCount }}</span></button>
              <button type="button" :aria-pressed="filter === 'accepted'" :disabled="savingAcceptance" @click="changeFilter('accepted')">Accepted</button>
            </div>
            <label v-if="projects.length > 1" class="project-filter">Project
              <select v-model="projectFilter" :disabled="savingAcceptance || !!review"><option value="">All projects</option><option v-for="project in projects" :key="project">{{ project }}</option></select>
            </label>
          </div>
          <div class="list-heading"><h2>{{ filter === 'needs-you' ? 'Your decisions' : filter === 'accepted' ? 'Accepted work' : 'Latest updates' }}</h2><span>Newest first · report time</span></div>
          <ul class="task-list" aria-label="Tasks, newest report first">
            <li v-for="task in visibleTasks" :key="taskKey(task)">
              <button class="task-item" type="button" :aria-pressed="taskKey(task) === selectedKey" :disabled="savingAcceptance" @click="selectTask(task)">
                <span class="task-main"><span class="task-project">{{ task.project }}</span><strong>{{ task.task_title }}</strong><span class="task-state"><i class="status-dot" :class="task.state"></i>{{ stateLabels[task.state] }}<span v-if="task.state === 'running'"> · {{ actorLabels[task.next_actor] || 'Owner unreported' }}</span><span v-if="uncertain(task)" class="warning-text"> · Needs a status check</span></span></span>
                <time class="task-time" :datetime="observationISO(task)" :title="reportAge(task)"><span>{{ rowDate(task) }}</span><strong>{{ rowTime(task) }}</strong><span v-if="lastObserved(task) !== null && (lastObserved(task) ?? 0) > Date.now() / 1000" class="warning-text">Future clock</span></time>
              </button>
            </li>
          </ul>
          <p v-if="!visibleTasks.length" class="empty-filter">{{ filter === 'accepted' ? 'No current reports accepted yet. Earlier decisions remain in each task’s details.' : 'No tasks match this view.' }}</p>
        </section>
        <section v-if="selectedTask" :key="taskKey(selectedTask)" class="task-inspector" aria-label="Task details">
          <button class="back-button" type="button" :disabled="savingAcceptance" @click="mobileDetail = false">← Back to updates</button>
          <div class="inspector-heading"><span class="state-label"><i class="status-dot" :class="selectedTask.state"></i>{{ stateLabels[selectedTask.state] }}</span><span class="muted">{{ selectedTask.project }}</span></div>
          <h2 ref="detailHeading" tabindex="-1">{{ selectedTask.task_title }}</h2>
          <div class="report-time"><span>{{ reportAge(selectedTask) }}</span><time v-if="lastObserved(selectedTask) !== null" :datetime="observationISO(selectedTask)">{{ timestamp((lastObserved(selectedTask) ?? 0)) }}</time></div>
          <p v-if="uncertain(selectedTask)" class="warning-text">Needs a status check{{ selectedTask.source_unavailable ? " · Evidence unavailable" : "" }}</p>
          <template v-if="!review || review.project !== selectedTask.project || review.task !== selectedTask.task">
            <section class="report-summary" aria-label="Latest reported progress"><h3>Where it stands</h3><p>{{ taskSummary(selectedTask) }}</p></section>
            <section class="next-action"><h3>{{ nextActionLabel(selectedTask) }}</h3><p>{{ nextStep(selectedTask) }}</p>
              <div v-if="overview.can_accept" class="review-actions"><button v-for="run in reviewableRuns(selectedTask)" :key="run.receipt.id" type="button" class="primary-button" :disabled="savingAcceptance" @click="startReview(selectedTask, run)">{{ reviewableRuns(selectedTask).length === 1 ? 'Review change' : `Review ${run.receipt.report.source} report` }}</button></div>
            </section>
          </template>
          <section v-if="review?.project === selectedTask.project && review.task === selectedTask.task" class="review-panel" :aria-label="`Review ${review.title}`">
            <h3>Check this reported change</h3>
            <p>{{ review.summary }}</p>
            <p class="muted">Report from {{ timestamp(review.observedAt) }}. Acceptance covers this report only, not the whole project or later work.</p>
            <ul v-if="review.evidence.length" aria-label="Evidence to review">
              <li v-for="reference in review.evidence" :key="reference">{{ reference }}
                <button class="evidence-open" v-if="review.openable.includes(reference)" type="button" :aria-label="`Open ${reference}`" @click="openEvidence(review.receiptId, reference)">Open</button>
                <span v-if="openStatus?.receiptId === review.receiptId && openStatus.reference === reference" role="status"> {{ openStatus.message }}</span>
              </li>
            </ul>
            <p v-else>No evidence was attached. Check the result before accepting.</p>
            <p v-if="!canConfirmReview && !loading" class="warning-text" role="alert">This report is no longer available for acceptance. Refresh, close this review and review the current change.</p>
            <p v-if="reviewError" class="warning-text" role="alert">{{ reviewError }}</p>
            <p>Does this reported change meet what you asked for?</p>
            <div class="review-actions">
              <button type="button" class="primary-button" :disabled="!canConfirmReview || savingAcceptance" @click="acceptReview">{{ savingAcceptance ? "Saving acceptance…" : "Accept this change" }}</button>
              <button type="button" class="quiet-button" :disabled="savingAcceptance" @click="cancelReview">Keep unaccepted</button>
            </div>
            <p class="muted">If it needs changes, keep it unaccepted and tell the agent what to fix.</p>
          </section>
          <details class="provenance">
            <summary>Reports & evidence</summary>
            <template v-for="superseded in [false, true]" :key="String(superseded)">
              <component :is="superseded ? 'details' : 'div'" v-if="selectedTask.runs.some(run => run.superseded === superseded)">
                <summary v-if="superseded">Previous runs ({{ selectedTask.runs.filter(run => run.superseded).length }})</summary>
                <div v-for="run in selectedTask.runs.filter(item => item.superseded === superseded)" :key="run.receipt.id" class="run-row">
                  <p><strong>{{ run.receipt.report.source }}</strong> · {{ run.superseded ? "Handed over" : run.accepted ? "Accepted" : stateLabels[run.receipt.report.state] }}<span v-if="run.stale"> · stale</span></p>
                  <p>{{ run.receipt.report.summary || "No summary reported." }}</p>
                  <p>{{ superseded || run.accepted ? "Previous next step" : "Next" }} · {{ run.receipt.report.next_step || "No next step reported." }} · {{ actorLabels[run.receipt.report.next_actor] }}</p>
                  <dl>
                    <dt>Session</dt><dd>{{ run.receipt.report.session_id }}</dd>
                    <dt>Run</dt><dd>{{ run.receipt.report.run_id }}</dd>
                    <dt>Worktree</dt><dd>{{ run.receipt.report.worktree }}</dd>
                    <dt>Revision</dt><dd>{{ run.receipt.report.revision }}</dd>
                    <dt>Observed</dt><dd>{{ timestamp(run.receipt.report.observed_at) }}</dd>
                    <dt>Received</dt><dd>{{ timestamp(run.receipt.received_at) }}</dd>
                  </dl>
                  <p v-if="run.receipt.report.evidence_status === 'unavailable'" class="warning-text">Evidence unavailable</p>
                  <ul v-if="run.receipt.report.evidence.length" aria-label="Evidence references">
                    <li v-for="reference in run.receipt.report.evidence" :key="reference">{{ reference }}<template v-if="run.openable_evidence?.includes(reference)">
                      <button type="button" :aria-label="`Open ${reference}`" @click="openEvidence(run.receipt.id, reference)">Open</button>
                    </template><span v-if="openStatus?.receiptId === run.receipt.id && openStatus.reference === reference" role="status"> {{ openStatus.message }}</span></li>
                  </ul>
                  <p v-else class="muted">No evidence references supplied.</p>
                </div>
              </component>
            </template>
          </details>
        <details v-for="decision in selectedTask.acceptances" :key="decision.receipt_id" class="work-guidance acceptance-history">
          <summary>Accepted · {{ decision.scope }} · {{ decision.accepted_by }}</summary>
          <details>
            <summary>Acceptance evidence</summary>
            <p>Recorded {{ timestamp(decision.accepted_at) }} · Receipt {{ decision.receipt_id }} · {{ selectedTask.project }}</p>
            <ul><li v-for="reference in decision.evidence" :key="reference">{{ reference }}</li></ul>
          </details>
        </details>
        <section v-if="selectedTask.recommendation" class="work-guidance" :aria-label="`Recommended next: ${selectedTask.recommendation.task_title}`">
          <p class="muted">Recommended next · {{ selectedTask.project }}</p>
          <strong>{{ selectedTask.recommendation.task_title }}</strong>
          <p>{{ selectedTask.recommendation.reason }}</p>
          <p>{{ actorLabels[selectedTask.recommendation.next_actor] }} acts next · Recommendation only — not authorised or started.</p>
          <button type="button" class="quiet-button" @click="copyPrompt(selectedTask.recommendation)">Copy session prompt</button>
          <p v-if="copyStatus?.prompt === selectedTask.recommendation.prompt" role="status">{{ copyStatus.message }}</p>
          <textarea readonly rows="5" aria-label="Recommended session prompt" :value="selectedTask.recommendation.prompt"></textarea>
          <details>
            <summary>Recommendation evidence</summary>
            <p>Checked {{ timestamp(selectedTask.recommendation.checked_at) }} · Task {{ selectedTask.recommendation.task }}</p>
            <ul><li v-for="reference in selectedTask.recommendation.evidence" :key="reference">{{ reference }}</li></ul>
          </details>
        </section>

        </section>
      </div>
      <footer class="work-footer"><span>Reported progress, not independently confirmed completion.</span><details><summary>About this view</summary><p>Only work that reports here is shown. Unreported work and Linear/RedPen reconciliation are not covered. Times show the latest agent observation; receipt diagnostics stay in task details. Checks refresh every 10 seconds.</p></details></footer>
    </template>
  </div>
</template>

<style scoped>
.work-view { --work-ink: var(--color-text-primary); --work-muted: var(--color-text-secondary); --work-line: var(--glass-border); --work-accent: var(--color-accent); color: var(--work-ink); padding: 52px 36px 24px; max-width: 1480px; margin: 0 auto; font-size: 14px; line-height: 1.5; }
h1, h2, h3, p { margin: 0; }
h1 { font-size: 28px; font-weight: 650; letter-spacing: -0.025em; }
h2 { font-weight: 600; }
h3 { font-size: 13px; font-weight: 650; }
button, select, textarea { font: inherit; }
button { cursor: pointer; }
button:disabled { cursor: default; opacity: .5; }
button:focus-visible, summary:focus-visible, select:focus-visible, textarea:focus-visible { outline: 2px solid var(--work-accent); outline-offset: 3px; }
.muted, .report-time, .work-footer { color: var(--work-muted); font-size: 12px; }
.work-header, .refresh-control, .status-line, .list-heading, .inspector-heading { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
.work-header .muted { font-size: 14px; margin-top: 3px; }
.refresh-control { gap: 12px; }
.quiet-button, .primary-button { min-height: 36px; border-radius: 6px; padding: 7px 14px; border: 1px solid var(--work-line); background: var(--colour-surface-input); color: var(--work-ink); font-weight: 550; }
.quiet-button:hover:not(:disabled) { background: var(--colour-surface-overlay); }
.primary-button { color: #fff; background: var(--work-accent); border-color: var(--work-accent); }
.primary-button:hover:not(:disabled) { background: var(--color-accent); }
.status-line { justify-content: flex-start; margin: 22px 0 26px; gap: 24px; color: var(--work-muted); }
.status-line > span { display: inline-flex; align-items: center; gap: 7px; }
.status-line strong { color: var(--work-ink); font-weight: 600; }
.status-dot { width: 6px; height: 6px; display: inline-block; flex: 0 0 auto; border-radius: 50%; background: var(--color-text-secondary); }
.status-dot.running { background: var(--color-accent); }
.status-dot.implemented, .status-dot.needs_user { background: var(--colour-warning); }
.status-dot.accepted { background: var(--colour-success); }
.status-dot.conflict, .status-dot.reporting_missing { background: var(--colour-warning); }
.work-layout { display: grid; grid-template-columns: minmax(340px, 1fr) minmax(370px, 1fr); border-top: 1px solid var(--work-line); align-items: start; }
.task-browser { min-width: 0; padding-right: 28px; }
.browser-toolbar { padding: 20px 0 18px; display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
.view-filters { display: flex; gap: 4px; }
.view-filters button { padding: 7px 10px; border-radius: 5px; color: var(--work-muted); font-size: 13px; }
.view-filters button:hover { background: var(--colour-surface-overlay); }
.view-filters button[aria-pressed="true"] { color: var(--work-ink); background: var(--colour-surface-overlay); font-weight: 600; }
.view-filters span { margin-left: 5px; font-variant-numeric: tabular-nums; }
.project-filter { color: var(--work-muted); font-size: 12px; display: flex; gap: 6px; align-items: center; }
.project-filter select { border: 1px solid var(--work-line); border-radius: 4px; padding: 4px; max-width: 170px; background: var(--colour-surface-input); }
.list-heading { margin-bottom: 10px; gap: 8px; }
.list-heading h2 { font-size: 14px; }
.list-heading > span { font-size: 11px; color: var(--work-muted); }
.task-list { list-style: none; padding: 0; margin: 0; }
.task-list li { border-bottom: 1px solid var(--work-line); }
.task-item { display: flex; width: 100%; text-align: left; align-items: flex-start; justify-content: space-between; padding: 17px 14px; gap: 18px; border-radius: 6px; color: var(--work-ink); }
.task-item:hover { background: var(--colour-surface-overlay); }
.task-item[aria-pressed="true"] { background: var(--colour-surface-overlay); }
.task-main { min-width: 0; display: grid; gap: 5px; }
.task-main > strong { font-weight: 600; font-size: 14px; line-height: 1.4; overflow-wrap: anywhere; }
.task-project { font-size: 11px; color: var(--work-muted); }
.task-state { font-size: 12px; color: var(--work-muted); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.task-time { display: grid; text-align: right; gap: 3px; font-variant-numeric: tabular-nums; white-space: nowrap; font-size: 11px; color: var(--work-muted); padding-top: 2px; }
.task-time strong { font-weight: 500; font-size: 13px; color: var(--work-ink); }
.task-inspector { border-left: 1px solid var(--work-line); padding: 25px 0 30px 30px; min-width: 0; position: sticky; top: 0; max-height: calc(100vh - 90px); overflow-y: auto; overflow-wrap: anywhere; }
.inspector-heading { font-size: 12px; margin-bottom: 16px; }
.state-label { display: inline-flex; align-items: center; gap: 7px; }
.task-inspector > h2 { font-size: 23px; line-height: 1.25; letter-spacing: -0.02em; max-width: 30ch; outline: none; }
.report-time { display: flex; flex-direction: column; gap: 2px; margin: 13px 0 28px; font-variant-numeric: tabular-nums; }
.report-summary p { margin-top: 9px; font-size: 14px; line-height: 1.65; max-width: 60ch; }
.next-action { margin-top: 24px; padding: 18px 0; border-top: 1px solid var(--work-line); }
.next-action h3 { margin-bottom: 7px; }
.next-action p { color: var(--work-muted); }
.review-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 15px; }
.review-panel { display: grid; gap: 14px; padding-bottom: 22px; }
.review-panel h3 { font-size: 16px; }
.review-panel > p { line-height: 1.65; }
summary { cursor: pointer; font-size: 13px; font-weight: 500; }
details > p, details > ul { margin-top: 12px; }
.provenance, .acceptance-history { border-top: 1px solid var(--work-line); padding: 15px 0; }
.run-row { display: grid; gap: 10px; font-size: 12px; padding: 18px 0; }
.run-row + .run-row { border-top: 1px solid var(--work-line); }
dl { display: grid; grid-template-columns: max-content minmax(0,1fr); gap: 6px 12px; margin: 0; }
dt { color: var(--work-muted); } dd { margin: 0; }
ul:not(.task-list) { padding-left: 18px; list-style: disc; margin: 0; }
.run-row button, .evidence-open { color: var(--work-accent); text-decoration: underline; padding: 3px 5px; margin-left: 4px; }
.work-guidance { border-top: 1px solid var(--work-line); padding: 16px 0; }
.work-guidance > * + * { margin-top: 10px; }
.work-guidance textarea { width: 100%; background: var(--colour-surface-input); color: var(--work-ink); border: 1px solid var(--work-line); border-radius: 4px; padding: 10px; resize: vertical; }
.work-footer { display: flex; justify-content: space-between; gap: 20px; border-top: 1px solid var(--work-line); padding-top: 16px; }
.work-footer details { max-width: 55ch; text-align: right; }
.work-footer summary { font-size: 12px; }
.work-warning, .review-message { padding: 14px 0; }
.warning-text, .work-warning { color: var(--colour-warning); }
.review-message { color: var(--colour-success); }
.empty-state, .loading-state { padding: 45px 0; max-width: 55ch; }
.empty-filter { padding: 30px 14px; color: var(--work-muted); }
.back-button { display: none; }
@media (max-width: 850px) {
  .work-view { padding: 52px 22px 18px; }
  .work-layout { grid-template-columns: minmax(260px, 1fr) minmax(300px, 1fr); }
  .task-browser { padding-right: 18px; }
  .task-inspector { padding-left: 20px; }
  .task-item { padding: 14px 10px; gap: 12px; }
  .list-heading { flex-wrap: wrap; }
}
@media (max-width: 900px) {
  .work-view { padding: 52px 18px 15px; }
  .work-header h1 { font-size: 25px; }
  .refresh-control > span { display: none; }
  .status-line { gap: 15px; flex-wrap: wrap; font-size: 12px; margin: 18px 0; }
  .work-layout { display: block; }
  .task-browser { padding-right: 0; }
  .task-inspector { position: static; max-height: none; border: 0; padding: 18px 0; }
  .work-view[data-detail="false"] .task-inspector, .work-view[data-detail="true"] .task-browser { display: none; }
  .back-button { display: block; color: var(--work-accent); margin-bottom: 23px; padding: 5px 0; }
  .work-footer { flex-direction: column; gap: 8px; }
  .work-footer details { text-align: left; }
}
</style>
