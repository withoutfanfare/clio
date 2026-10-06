import { invoke } from "@tauri-apps/api/core";

export type WorkState = "running" | "waiting" | "stopped" | "implemented";
export type NextActor = "agent" | "user" | "other" | "none";
export type TaskState = WorkState | "needs_user" | "reporting_missing" | "conflict" | "accepted";
export interface WorkReport {
  project: string;
  task: string;
  task_title: string;
  source: string;
  session_id: string;
  run_id: string;
  sequence: number;
  observed_at: number;
  worktree: string;
  revision: string;
  state: WorkState;
  summary: string;
  next_step: string;
  next_actor: NextActor;
  evidence: string[];
  evidence_status: "current" | "unavailable";
  supersedes: { source: string; run_id: string } | null;
}
export interface Acceptance {
  receipt_id: number;
  scope: string;
  accepted_by: string;
  accepted_at: number;
  evidence: string[];
}
export interface Recommendation {
  project: string;
  parent_task: string;
  task: string;
  task_title: string;
  reason: string;
  next_actor: NextActor;
  prompt: string;
  evidence: string[];
  checked_at: number;
}
export interface RunView {
  openable_evidence?: string[];
  receipt: { id: number; received_at: number; report: WorkReport };
  superseded: boolean;
  accepted?: boolean;
  stale: boolean;
}
export interface TaskView {
  project: string;
  task: string;
  task_title: string;
  state: TaskState;
  next_actor: NextActor;
  stale: boolean;
  source_unavailable: boolean;
  runs: RunView[];
  acceptances?: Acceptance[];
  recommendation?: Recommendation | null;
}
export interface WorkOverview { tasks: TaskView[]; can_accept?: boolean }

export function workOverview(project: string | null = null, staleAfterSecs: number | null = null): Promise<WorkOverview> {
  return invoke("cmd_work_overview", { project, staleAfterSecs });
}
