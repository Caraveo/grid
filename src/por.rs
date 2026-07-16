//! Proof-of-Resource scoring + emission allocation (token-spec aligned).

/// Default weights from token spec: compute, uptime, efficiency, fidelity.
pub const W_COMPUTE: f64 = 0.55;
pub const W_UPTIME: f64 = 0.15;
pub const W_EFFICIENCY: f64 = 0.10;
pub const W_FIDELITY: f64 = 0.20;

/// Base whale ceiling (hyperscaler). Effective γ = max(BASE_GAMMA, 1/N).
pub const BASE_GAMMA: f64 = 0.05;

/// Inclusion pool fraction for little miners (class S).
pub const INCLUSION_FRAC: f64 = 0.10;

#[derive(Debug, Clone, Copy)]
pub struct PorInputs {
    pub compute: f64,
    pub uptime: f64,
    pub efficiency: f64,
    pub fidelity: f64,
    pub reputation: f64,
}

impl Default for PorInputs {
    fn default() -> Self {
        Self {
            compute: 0.0,
            uptime: 1.0,
            efficiency: 0.5,
            fidelity: 0.5,
            reputation: 1.0,
        }
    }
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

fn clamp_rep(r: f64) -> f64 {
    r.clamp(0.5, 1.5)
}

/// R = w_c·C + w_u·U + w_e·E + w_f·F
pub fn resource_score(i: &PorInputs) -> f64 {
    W_COMPUTE * clamp01(i.compute)
        + W_UPTIME * clamp01(i.uptime)
        + W_EFFICIENCY * clamp01(i.efficiency)
        + W_FIDELITY * clamp01(i.fidelity)
}

/// S = R · ρ
pub fn effective_score(i: &PorInputs) -> f64 {
    resource_score(i) * clamp_rep(i.reputation)
}

pub fn split_emission(epoch_mint: f64) -> (f64, f64) {
    let inc = epoch_mint * INCLUSION_FRAC;
    (epoch_mint - inc, inc)
}

/// Inputs for proportional allocation.
#[derive(Debug, Clone)]
pub struct NodeScore {
    pub node_id: String,
    pub cluster_id: String,
    pub score: f64,
    pub class_s: bool,
}

/**
 * Water-fill proportional pay with per-cluster ceiling.
 * effective_gamma = max(BASE_GAMMA, 1/N) so small networks still pay out fully.
 */
pub fn allocate_proportional(nodes: &[NodeScore], pool: f64) -> Vec<(String, f64)> {
    if nodes.is_empty() || pool <= 0.0 {
        return nodes.iter().map(|n| (n.node_id.clone(), 0.0)).collect();
    }

    // Aggregate by cluster
    use std::collections::HashMap;
    let mut cluster_score: HashMap<String, f64> = HashMap::new();
    let mut members: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.score <= 0.0 {
            continue;
        }
        *cluster_score.entry(n.cluster_id.clone()).or_insert(0.0) += n.score;
        members.entry(n.cluster_id.clone()).or_default().push(i);
    }

    let clusters: Vec<String> = cluster_score.keys().cloned().collect();
    let n_c = clusters.len().max(1);
    let gamma = BASE_GAMMA.max(1.0 / n_c as f64);
    let max_cluster = gamma * pool;

    let mut cluster_pay: HashMap<String, f64> = clusters.iter().map(|c| (c.clone(), 0.0)).collect();
    let mut remaining = pool;
    let mut open: std::collections::HashSet<String> = clusters.iter().cloned().collect();

    for _ in 0..n_c + 3 {
        if open.is_empty() || remaining <= 1e-12 {
            break;
        }
        let open_score: f64 = open.iter().map(|c| cluster_score[c]).sum();
        if open_score <= 0.0 {
            break;
        }

        let mut any_capped = false;
        let tentative: HashMap<String, f64> = open
            .iter()
            .map(|c| (c.clone(), remaining * cluster_score[c] / open_score))
            .collect();

        for c in open.clone() {
            let already = cluster_pay[&c];
            let room = (max_cluster - already).max(0.0);
            if tentative[&c] > room + 1e-12 {
                cluster_pay.insert(c.clone(), already + room);
                remaining -= room;
                open.remove(&c);
                any_capped = true;
            }
        }

        if !any_capped {
            for c in &open {
                *cluster_pay.get_mut(c).unwrap() += tentative[c];
            }
            break;
        }
    }

    let mut out = vec![(String::new(), 0.0); nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        out[i] = (n.node_id.clone(), 0.0);
        let Some(idxs) = members.get(&n.cluster_id) else {
            continue;
        };
        let pay = cluster_pay.get(&n.cluster_id).copied().unwrap_or(0.0);
        let m_score: f64 = idxs.iter().map(|&j| nodes[j].score).sum::<f64>().max(1e-12);
        out[i].1 = pay * n.score / m_score;
    }
    out
}

/// Inclusion pool: class-S nodes only, proportional to score.
pub fn allocate_inclusion(nodes: &[NodeScore], pool: f64) -> Vec<(String, f64)> {
    let little: Vec<&NodeScore> = nodes.iter().filter(|n| n.class_s && n.score > 0.0).collect();
    if little.is_empty() || pool <= 0.0 {
        return nodes.iter().map(|n| (n.node_id.clone(), 0.0)).collect();
    }
    let total: f64 = little.iter().map(|n| n.score).sum();
    nodes
        .iter()
        .map(|n| {
            if n.class_s && n.score > 0.0 {
                (n.node_id.clone(), pool * n.score / total)
            } else {
                (n.node_id.clone(), 0.0)
            }
        })
        .collect()
}

/// Build PoR inputs from job counters (Phase 1 heuristic).
pub fn inputs_from_jobs(done: u64, failed: u64, online: bool) -> PorInputs {
    let total = done + failed;
    let fidelity = if total == 0 {
        0.5
    } else {
        done as f64 / total as f64
    };
    let compute = (done as f64 / 10.0).min(1.0);
    PorInputs {
        compute,
        uptime: if online { 1.0 } else { 0.2 },
        efficiency: 0.5,
        fidelity,
        reputation: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_weighted() {
        let high = resource_score(&PorInputs {
            compute: 1.0,
            uptime: 0.0,
            efficiency: 0.0,
            fidelity: 0.0,
            reputation: 1.0,
        });
        assert!(high > 0.5);
    }

    #[test]
    fn whale_capped_with_many_peers() {
        let mut nodes = vec![NodeScore {
            node_id: "whale".into(),
            cluster_id: "google".into(),
            score: 1000.0,
            class_s: false,
        }];
        for i in 0..30 {
            nodes.push(NodeScore {
                node_id: format!("h{i}"),
                cluster_id: format!("home{i}"),
                score: 1.0,
                class_s: true,
            });
        }
        let pay = allocate_proportional(&nodes, 100.0);
        let whale = pay.iter().find(|(id, _)| id == "whale").unwrap().1;
        assert!(whale <= 5.0 + 1e-6, "whale={whale}");
        let homes: f64 = pay.iter().filter(|(id, _)| id != "whale").map(|(_, p)| p).sum();
        assert!(homes >= 94.0, "homes={homes}");
    }

    #[test]
    fn small_network_pays_out() {
        let nodes = vec![
            NodeScore {
                node_id: "a".into(),
                cluster_id: "google".into(),
                score: 100.0,
                class_s: false,
            },
            NodeScore {
                node_id: "b".into(),
                cluster_id: "home1".into(),
                score: 1.0,
                class_s: true,
            },
            NodeScore {
                node_id: "c".into(),
                cluster_id: "home2".into(),
                score: 1.0,
                class_s: true,
            },
        ];
        let pay = allocate_proportional(&nodes, 100.0);
        let sum: f64 = pay.iter().map(|(_, p)| p).sum();
        assert!((sum - 100.0).abs() < 1e-6, "sum={sum}");
    }

    #[test]
    fn inclusion_only_class_s() {
        let nodes = vec![
            NodeScore {
                node_id: "big".into(),
                cluster_id: "dc".into(),
                score: 10.0,
                class_s: false,
            },
            NodeScore {
                node_id: "lil".into(),
                cluster_id: "home".into(),
                score: 1.0,
                class_s: true,
            },
        ];
        let pay = allocate_inclusion(&nodes, 100.0);
        assert_eq!(pay.iter().find(|(id, _)| id == "big").unwrap().1, 0.0);
        assert!((pay.iter().find(|(id, _)| id == "lil").unwrap().1 - 100.0).abs() < 1e-9);
    }
}
