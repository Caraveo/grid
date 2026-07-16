/**
 * Proof-of-Resource scoring v0 — matches token spec weights spirit,
 * simplified for MVP (no energy telemetry yet).
 */

export interface PorInputs {
  /** Normalized 0..1 completed work units this epoch. */
  compute: number;
  /** Normalized 0..1 uptime / availability. */
  uptime: number;
  /** Normalized 0..1 efficiency proxy (optional; default 0.5). */
  efficiency?: number;
  /** Normalized 0..1 fidelity (success / challenge pass rate). */
  fidelity: number;
  /** Reputation multiplier in [0.5, 1.5]. */
  reputation?: number;
}

export interface PorWeights {
  wC: number;
  wU: number;
  wE: number;
  wF: number;
}

/** Token-spec default weights. */
export const DEFAULT_WEIGHTS: PorWeights = {
  wC: 0.55,
  wU: 0.15,
  wE: 0.1,
  wF: 0.2,
};

function clamp01(x: number): number {
  if (Number.isNaN(x)) return 0;
  return Math.min(1, Math.max(0, x));
}

function clampRep(r: number): number {
  return Math.min(1.5, Math.max(0.5, r));
}

/** R_i = w_c C + w_u U + w_e E + w_f F */
export function resourceScore(
  inputs: PorInputs,
  weights: PorWeights = DEFAULT_WEIGHTS
): number {
  const C = clamp01(inputs.compute);
  const U = clamp01(inputs.uptime);
  const E = clamp01(inputs.efficiency ?? 0.5);
  const F = clamp01(inputs.fidelity);
  return weights.wC * C + weights.wU * U + weights.wE * E + weights.wF * F;
}

/** S_i = R_i * rho */
export function effectiveScore(
  inputs: PorInputs,
  weights: PorWeights = DEFAULT_WEIGHTS
): number {
  const R = resourceScore(inputs, weights);
  const rho = clampRep(inputs.reputation ?? 1);
  return R * rho;
}

/**
 * Proportional emission with per-cluster ceiling.
 * Base gamma (default 5%) floors up to 1/N when few clusters are active so
 * early networks still distribute the full pool, while large N enforces the
 * hyperscaler ceiling from the token spec.
 */
export function allocateProportional(
  scores: Record<string, number>,
  pool: number,
  clusterOf: Record<string, string>,
  gamma = 0.05
): Record<string, number> {
  const out: Record<string, number> = {};
  for (const id of Object.keys(scores)) out[id] = 0;

  const ids = Object.keys(scores).filter((id) => (scores[id] ?? 0) > 0);
  if (ids.length === 0 || pool <= 0) return out;

  const clusterScore: Record<string, number> = {};
  const members: Record<string, string[]> = {};
  for (const id of ids) {
    const c = clusterOf[id] ?? id;
    clusterScore[c] = (clusterScore[c] ?? 0) + (scores[id] ?? 0);
    (members[c] ??= []).push(id);
  }

  const clusters = Object.keys(clusterScore);
  const n = clusters.length;
  // Dynamic ceiling: never stricter than equal split when N is small
  const effectiveGamma = Math.max(gamma, 1 / n);
  const maxCluster = effectiveGamma * pool;

  const clusterPay: Record<string, number> = {};
  for (const c of clusters) clusterPay[c] = 0;

  let remaining = pool;
  const open = new Set(clusters);

  for (let iter = 0; iter < n + 3 && open.size > 0 && remaining > 1e-12; iter++) {
    const openScore = [...open].reduce((s, c) => s + (clusterScore[c] ?? 0), 0);
    if (openScore <= 0) break;

    let anyCapped = false;
    const tentative: Record<string, number> = {};
    for (const c of open) {
      tentative[c] = (remaining * (clusterScore[c] ?? 0)) / openScore;
    }
    for (const c of [...open]) {
      const already = clusterPay[c] ?? 0;
      const room = Math.max(0, maxCluster - already);
      if ((tentative[c] ?? 0) > room + 1e-12) {
        clusterPay[c] = already + room;
        remaining -= room;
        open.delete(c);
        anyCapped = true;
      }
    }
    if (!anyCapped) {
      for (const c of open) {
        clusterPay[c] = (clusterPay[c] ?? 0) + (tentative[c] ?? 0);
      }
      remaining = 0;
      break;
    }
  }

  for (const c of clusters) {
    const pay = clusterPay[c] ?? 0;
    const ms = members[c] ?? [];
    const mScore = ms.reduce((s, id) => s + (scores[id] ?? 0), 0) || 1;
    for (const id of ms) {
      out[id] = (pay * (scores[id] ?? 0)) / mScore;
    }
  }
  return out;
}

/** Split epoch mint: 90% prop / 10% inclusion (little miners). */
export function splitEmission(epochMint: number, inclusionFrac = 0.1) {
  const inc = epochMint * inclusionFrac;
  return { proportional: epochMint - inc, inclusion: inc };
}
