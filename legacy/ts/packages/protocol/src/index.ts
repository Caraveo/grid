import { createHash } from "node:crypto";

/** Allowlisted job kinds for MVP — expand only with sandbox review. */
export type JobKind = "echo" | "hash_file";

export type JobStatus =
  | "queued"
  | "assigned"
  | "running"
  | "completed"
  | "failed"
  | "verified"
  | "rejected";

export interface JobSpec {
  id: string;
  kind: JobKind;
  /** Opaque payload (string or base64). Keep small in MVP. */
  payload: string;
  createdAt: string;
  /** Max runtime seconds on node. */
  timeoutSec: number;
}

export interface JobResult {
  jobId: string;
  nodeId: string;
  ok: boolean;
  /** Canonical output string for hashing. */
  output: string;
  startedAt: string;
  finishedAt: string;
  /** Wall time ms. */
  durationMs: number;
  error?: string;
}

export interface NodeHeartbeat {
  nodeId: string;
  wallet?: string;
  /** Operator-reported capacity class. */
  class: "S" | "M" | "L";
  gpuModel?: string;
  maxConcurrent: number;
  /** Epoch ms. */
  ts: number;
}

export function sha256(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/** Result commitment used by verifier v0. */
export function resultCommitment(result: JobResult): string {
  const canonical = [
    result.jobId,
    result.nodeId,
    result.ok ? "1" : "0",
    result.output,
    String(result.durationMs),
  ].join("|");
  return sha256(canonical);
}

export function newJobId(): string {
  return `job_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

export function newNodeId(): string {
  return `node_${Math.random().toString(36).slice(2, 12)}`;
}
