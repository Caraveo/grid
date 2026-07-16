/**
 * `grid node` — run a GRID miner node (useful mining agent).
 */
import { createHash, randomUUID } from "node:crypto";
import { envOr, flag, opt } from "../lib/args.js";

export interface NodeOptions {
  coordinator: string;
  nodeId: string;
  class: "S" | "M" | "L";
  gpuModel: string;
  pollMs: number;
  maxConcurrent: number;
}

interface Job {
  id: string;
  kind: "echo" | "hash_file";
  payload: string;
  timeoutSec: number;
}

function parseClass(v: string | undefined): "S" | "M" | "L" {
  const u = (v ?? "S").toUpperCase();
  if (u === "S" || u === "M" || u === "L") return u;
  throw new Error(`invalid --class ${v} (use S|M|L)`);
}

export function parseNodeOptions(argv: string[]): NodeOptions {
  return {
    coordinator: opt(argv, "--coordinator") ?? envOr("GRID_COORDINATOR", "http://127.0.0.1:8787"),
    nodeId: opt(argv, "--id") ?? envOr("GRID_NODE_ID", `node_${randomUUID().slice(0, 8)}`),
    class: parseClass(opt(argv, "--class") ?? process.env.GRID_NODE_CLASS),
    gpuModel: opt(argv, "--gpu") ?? envOr("GRID_GPU_MODEL", "cpu-demo"),
    pollMs: Number(opt(argv, "--poll-ms") ?? process.env.GRID_POLL_MS ?? "2000"),
    maxConcurrent: Number(opt(argv, "--max-concurrent") ?? "1"),
  };
}

async function post(
  coordinator: string,
  path: string,
  body: unknown,
  allow204 = false
): Promise<any> {
  const res = await fetch(`${coordinator}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status === 204 && allow204) return null;
  const text = await res.text();
  if (!res.ok) throw new Error(`${path} ${res.status}: ${text}`);
  return text ? JSON.parse(text) : null;
}

function runJob(job: Job): { ok: boolean; output: string; durationMs: number; error?: string } {
  const t0 = Date.now();
  try {
    if (job.kind === "echo") {
      return { ok: true, output: job.payload, durationMs: Date.now() - t0 };
    }
    if (job.kind === "hash_file") {
      const output = createHash("sha256").update(job.payload, "utf8").digest("hex");
      return { ok: true, output, durationMs: Date.now() - t0 };
    }
    return { ok: false, output: "", durationMs: Date.now() - t0, error: "unknown kind" };
  } catch (e) {
    return { ok: false, output: "", durationMs: Date.now() - t0, error: String(e) };
  }
}

export async function runNode(opts: NodeOptions): Promise<void> {
  const { coordinator, nodeId, class: nodeClass, gpuModel, pollMs, maxConcurrent } = opts;

  console.log(`GRID node ${nodeId}`);
  console.log(`  coordinator  ${coordinator}`);
  console.log(`  class        ${nodeClass}`);
  console.log(`  gpu          ${gpuModel}`);
  console.log(`  (Ctrl+C to stop)`);

  const heartbeat = () =>
    post(coordinator, "/v1/nodes/heartbeat", {
      nodeId,
      class: nodeClass,
      gpuModel,
      maxConcurrent,
    });

  await heartbeat();
  setInterval(() => {
    heartbeat().catch((e) => console.error("heartbeat failed", e));
  }, 15_000);

  for (;;) {
    try {
      const claimed = await post(coordinator, "/v1/nodes/claim", { nodeId }, true);
      const job = claimed?.job as Job | null | undefined;
      if (!job) {
        await new Promise((r) => setTimeout(r, pollMs));
        continue;
      }
      console.log(`claimed ${job.id} kind=${job.kind}`);
      const result = runJob(job);
      const done = await post(coordinator, "/v1/jobs/complete", {
        jobId: job.id,
        nodeId,
        ok: result.ok,
        output: result.output,
        durationMs: result.durationMs,
      });
      const earn = done?.earnCredits;
      console.log(
        `finished ${job.id} verified=${done?.verified} earn=${typeof earn === "number" ? earn.toFixed(4) : earn}`
      );
    } catch (e) {
      console.error("loop error", e);
      await new Promise((r) => setTimeout(r, pollMs));
    }
  }
}

export function printNodeHelp(): void {
  console.log(`Usage: grid node [options]

Run a GRID miner node (join the network, claim jobs, earn).

Options:
  --coordinator <url>   Coordinator URL (default: http://127.0.0.1:8787)
  --id <nodeId>         Stable node id (default: random)
  --class <S|M|L>       Capacity class (default: S)
  --gpu <name>          GPU model label (default: cpu-demo)
  --poll-ms <n>         Claim poll interval ms (default: 2000)
  --max-concurrent <n>  Max jobs at once (default: 1)

Env:
  GRID_COORDINATOR, GRID_NODE_ID, GRID_NODE_CLASS, GRID_GPU_MODEL, GRID_POLL_MS

Examples:
  grid node
  grid node --class M --gpu "RTX 3080"
  grid node --coordinator https://coord.example --id miner-garage-1
`);
}

/** `grid node` entry — supports bare `grid node` and `grid node start`. */
export async function nodeCommand(argv: string[]): Promise<void> {
  if (flag(argv, "--help") || flag(argv, "-h")) {
    printNodeHelp();
    return;
  }
  // allow: grid node | grid node start | grid node run
  const rest = argv[0] === "start" || argv[0] === "run" ? argv.slice(1) : argv;
  if (rest[0] === "help") {
    printNodeHelp();
    return;
  }
  const opts = parseNodeOptions(rest);
  await runNode(opts);
}
