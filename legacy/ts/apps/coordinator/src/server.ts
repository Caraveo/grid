/**
 * GRID Coordinator — MVP P1
 * In-memory job queue + node registry. Not production.
 */
import http from "node:http";
import { createHash, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

// Inline protocol types (avoid monorepo link friction for solo clone)
type JobKind = "echo" | "hash_file";
type JobStatus =
  | "queued"
  | "assigned"
  | "running"
  | "completed"
  | "failed"
  | "verified"
  | "rejected";

interface Job {
  id: string;
  kind: JobKind;
  payload: string;
  createdAt: string;
  timeoutSec: number;
  status: JobStatus;
  assignedNodeId?: string;
  result?: {
    nodeId: string;
    ok: boolean;
    output: string;
    durationMs: number;
    commitment: string;
  };
  earnCredits?: number;
}

interface NodeRec {
  nodeId: string;
  class: "S" | "M" | "L";
  gpuModel?: string;
  maxConcurrent: number;
  lastSeen: number;
  jobsDone: number;
  jobsFailed: number;
  earnTotal: number;
}

const PORT = Number(process.env.GRID_PORT ?? 8787);
const jobs = new Map<string, Job>();
const nodes = new Map<string, NodeRec>();
const queue: string[] = [];

// Fixed epoch mint for demo earn ledger (not on-chain yet)
const DEMO_EPOCH_MINT = 1000;
const WHALE_GAMMA = 0.05;

function json(res: http.ServerResponse, code: number, body: unknown) {
  res.writeHead(code, {
    "content-type": "application/json",
    "access-control-allow-origin": "*",
  });
  res.end(JSON.stringify(body, null, 2));
}

async function readBody(req: http.IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const c of req) chunks.push(c as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

function commitment(jobId: string, nodeId: string, ok: boolean, output: string, durationMs: number) {
  const canonical = [jobId, nodeId, ok ? "1" : "0", output, String(durationMs)].join("|");
  return createHash("sha256").update(canonical).digest("hex");
}

function porScore(n: NodeRec): number {
  const total = n.jobsDone + n.jobsFailed;
  const fidelity = total === 0 ? 0.5 : n.jobsDone / total;
  const compute = Math.min(1, n.jobsDone / 10);
  const uptime = Date.now() - n.lastSeen < 60_000 ? 1 : 0.2;
  return 0.55 * compute + 0.15 * uptime + 0.1 * 0.5 + 0.2 * fidelity;
}

/** Demo: allocate tiny earn credits after verified job using gamma spirit. */
function creditEarn(nodeId: string) {
  const scores: Record<string, number> = {};
  const clusterOf: Record<string, string> = {};
  for (const [id, n] of nodes) {
    scores[id] = porScore(n);
    clusterOf[id] = id; // MVP: node = cluster
  }
  const total = Object.values(scores).reduce((a, b) => a + b, 0) || 1;
  let share = (DEMO_EPOCH_MINT * 0.01 * (scores[nodeId] ?? 0)) / total; // 1% of mint per event demo
  const cap = DEMO_EPOCH_MINT * WHALE_GAMMA;
  share = Math.min(share, cap);
  const n = nodes.get(nodeId);
  if (n) {
    n.earnTotal += share;
    return share;
  }
  return 0;
}

const server = http.createServer(async (req, res) => {
  if (req.method === "OPTIONS") {
    res.writeHead(204, {
      "access-control-allow-origin": "*",
      "access-control-allow-methods": "GET,POST,OPTIONS",
      "access-control-allow-headers": "content-type",
    });
    res.end();
    return;
  }

  const url = new URL(req.url ?? "/", `http://127.0.0.1:${PORT}`);
  const path = url.pathname;

  try {
    if (req.method === "GET" && path === "/health") {
      return json(res, 200, { ok: true, service: "grid-coordinator", jobs: jobs.size, nodes: nodes.size });
    }

    if (req.method === "GET" && path === "/v1/stats") {
      return json(res, 200, {
        jobs: [...jobs.values()].map((j) => ({
          id: j.id,
          status: j.status,
          kind: j.kind,
          earnCredits: j.earnCredits,
        })),
        nodes: [...nodes.values()],
      });
    }

    if (req.method === "POST" && path === "/v1/jobs") {
      const body = JSON.parse((await readBody(req)) || "{}") as {
        kind?: JobKind;
        payload?: string;
        timeoutSec?: number;
      };
      const kind = body.kind ?? "echo";
      if (kind !== "echo" && kind !== "hash_file") {
        return json(res, 400, { error: "kind must be echo|hash_file (MVP allowlist)" });
      }
      const job: Job = {
        id: `job_${randomUUID().slice(0, 8)}`,
        kind,
        payload: body.payload ?? "hello-grid",
        createdAt: new Date().toISOString(),
        timeoutSec: body.timeoutSec ?? 60,
        status: "queued",
      };
      jobs.set(job.id, job);
      queue.push(job.id);
      return json(res, 201, job);
    }

    if (req.method === "GET" && path.startsWith("/v1/jobs/")) {
      const id = path.split("/").pop()!;
      const job = jobs.get(id);
      if (!job) return json(res, 404, { error: "not found" });
      return json(res, 200, job);
    }

    if (req.method === "POST" && path === "/v1/nodes/heartbeat") {
      const body = JSON.parse((await readBody(req)) || "{}") as {
        nodeId?: string;
        class?: "S" | "M" | "L";
        gpuModel?: string;
        maxConcurrent?: number;
      };
      const nodeId = body.nodeId ?? `node_${randomUUID().slice(0, 8)}`;
      const existing = nodes.get(nodeId);
      const rec: NodeRec = {
        nodeId,
        class: body.class ?? "S",
        gpuModel: body.gpuModel,
        maxConcurrent: body.maxConcurrent ?? 1,
        lastSeen: Date.now(),
        jobsDone: existing?.jobsDone ?? 0,
        jobsFailed: existing?.jobsFailed ?? 0,
        earnTotal: existing?.earnTotal ?? 0,
      };
      nodes.set(nodeId, rec);
      return json(res, 200, rec);
    }

    if (req.method === "POST" && path === "/v1/nodes/claim") {
      const body = JSON.parse((await readBody(req)) || "{}") as { nodeId?: string };
      if (!body.nodeId || !nodes.has(body.nodeId)) {
        return json(res, 400, { error: "heartbeat first" });
      }
      while (queue.length) {
        const id = queue.shift()!;
        const job = jobs.get(id);
        if (!job || job.status !== "queued") continue;
        job.status = "assigned";
        job.assignedNodeId = body.nodeId;
        return json(res, 200, { job });
      }
      return json(res, 204, { job: null });
    }

    if (req.method === "POST" && path === "/v1/jobs/complete") {
      const body = JSON.parse((await readBody(req)) || "{}") as {
        jobId?: string;
        nodeId?: string;
        ok?: boolean;
        output?: string;
        durationMs?: number;
      };
      const job = body.jobId ? jobs.get(body.jobId) : undefined;
      if (!job || !body.nodeId) return json(res, 400, { error: "bad job" });
      if (job.assignedNodeId && job.assignedNodeId !== body.nodeId) {
        return json(res, 403, { error: "not assignee" });
      }
      const ok = Boolean(body.ok);
      const output = body.output ?? "";
      const durationMs = body.durationMs ?? 0;
      const commit = commitment(job.id, body.nodeId, ok, output, durationMs);

      // Verifier v0: deterministic kinds re-checked server-side
      let verified = ok;
      if (job.kind === "echo") {
        verified = ok && output === job.payload;
      } else if (job.kind === "hash_file") {
        const expect = createHash("sha256").update(job.payload, "utf8").digest("hex");
        verified = ok && output === expect;
      }

      job.result = {
        nodeId: body.nodeId,
        ok,
        output,
        durationMs,
        commitment: commit,
      };
      job.status = verified ? "verified" : "rejected";

      const node = nodes.get(body.nodeId);
      if (node) {
        if (verified) node.jobsDone += 1;
        else node.jobsFailed += 1;
      }

      let earn = 0;
      if (verified) {
        earn = creditEarn(body.nodeId);
        job.earnCredits = earn;
      }

      return json(res, 200, { job, verified, earnCredits: earn });
    }

    json(res, 404, { error: "not found", path });
  } catch (e) {
    json(res, 500, { error: String(e) });
  }
});

server.listen(PORT, () => {
  console.log(`GRID coordinator listening on http://127.0.0.1:${PORT}`);
  console.log(`  GET  /health`);
  console.log(`  POST /v1/jobs`);
  console.log(`  POST /v1/nodes/heartbeat`);
  console.log(`  POST /v1/nodes/claim`);
  console.log(`  POST /v1/jobs/complete`);
});
