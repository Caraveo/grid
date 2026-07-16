//! TCP length-prefixed JSON P2P (minimal MVP).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::protocol::Message;

#[derive(Clone)]
pub struct PeerOptions {
    pub node_id: String,
    pub name: String,
    pub class: String,
    pub listen: String,
    pub connect: Vec<String>,
    pub score: f64,
    /// Genesis truth URL (e.g. http://127.0.0.1:9100) — source of ban list.
    pub genesis_url: Option<String>,
    /// Expected genesis pubkey hex (trust anchor).
    pub genesis_pubkey: Option<String>,
}

#[derive(Clone)]
struct PeerMeta {
    node_id: String,
    name: String,
    listen: String,
    class: String,
    score: f64,
    rtt_ms: Option<f64>,
}

struct State {
    peers: HashMap<String, PeerMeta>,
    known_addrs: HashSet<String>,
    /// peer_id → ban reason (from verified genesis truth only)
    banned: HashMap<String, String>,
    truth_epoch: u64,
}

type Shared = Arc<Mutex<State>>;

pub async fn run_peer(opts: PeerOptions) -> Result<()> {
    let listen_addr: SocketAddr = opts
        .listen
        .parse()
        .with_context(|| format!("bad --listen {}", opts.listen))?;

    let state: Shared = Arc::new(Mutex::new(State {
        peers: HashMap::new(),
        known_addrs: HashSet::new(),
        banned: HashMap::new(),
        truth_epoch: 0,
    }));

    for c in &opts.connect {
        state.lock().known_addrs.insert(normalize_addr(c));
    }

    // Initial + periodic genesis truth pull (ban list)
    if let Some(ref gurl) = opts.genesis_url {
        refresh_truth(gurl, opts.genesis_pubkey.as_deref(), &state).await?;
        let gurl = gurl.clone();
        let gpk = opts.genesis_pubkey.clone();
        let state_t = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(15));
            loop {
                tick.tick().await;
                if let Err(e) =
                    refresh_truth(&gurl, gpk.as_deref(), &state_t).await
                {
                    debug!("genesis truth refresh: {e}");
                }
            }
        });
    }

    let listener = TcpListener::bind(listen_addr).await?;
    println!("GRID P2P listening on {}", opts.listen);
    println!("  node_id  {}", opts.node_id);
    println!("  name     {}", opts.name);
    println!("  class    {}", opts.class);
    println!("  score    {:.1}", opts.score);
    if !opts.connect.is_empty() {
        println!("  dial     {}", opts.connect.join(", "));
    }
    if let Some(ref g) = opts.genesis_url {
        let ep = state.lock().truth_epoch;
        let nb = state.lock().banned.len();
        println!("  genesis  {g} (epoch={ep} bans={nb})");
    } else {
        println!("  genesis  (none — bans not enforced)");
    }
    println!("  (Ctrl+C to stop)\n");

    // Outbound dials (initial + periodic)
    {
        let opts = opts.clone();
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let targets: Vec<String> = {
                    let mut s = state.lock();
                    for c in &opts.connect {
                        s.known_addrs.insert(normalize_addr(c));
                    }
                    s.known_addrs
                        .iter()
                        .filter(|a| !addrs_equal(a, &opts.listen))
                        .cloned()
                        .collect()
                };
                for t in targets {
                    let already = state
                        .lock()
                        .peers
                        .values()
                        .any(|p| addrs_equal(&p.listen, &t));
                    if already {
                        continue;
                    }
                    let opts = opts.clone();
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = dial_and_session(t.clone(), opts, state).await {
                            debug!("dial {t}: {e}");
                        }
                    });
                }
                tokio::time::sleep(Duration::from_secs(8)).await;
            }
        });
    }

    // Status ticker
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                let s = state.lock();
                if s.peers.is_empty() {
                    println!("[p2p] peers: 0 (waiting for connections…)");
                } else {
                    println!("[p2p] peers: {}", s.peers.len());
                    for p in s.peers.values() {
                        let rtt = p
                            .rtt_ms
                            .map(|r| format!("{r:.1} ms"))
                            .unwrap_or_else(|| "—".into());
                        println!(
                            "       · {} ({}) class={} score={:.0} rtt={} @ {}",
                            p.name, p.node_id, p.class, p.score, rtt, p.listen
                        );
                    }
                }
            }
        });
    }

    loop {
        let (stream, remote) = listener.accept().await?;
        let opts = opts.clone();
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, remote.to_string(), opts, state, true).await
            {
                debug!("inbound {remote}: {e}");
            }
        });
    }
}

async fn dial_and_session(target: String, opts: PeerOptions, state: Shared) -> Result<()> {
    {
        let s = state.lock();
        if s.peers.values().any(|p| addrs_equal(&p.listen, &target)) {
            return Ok(());
        }
    }
    let stream = TcpStream::connect(&target)
        .await
        .with_context(|| format!("connect {target}"))?;
    info!("connected → {target}");
    println!("[p2p] dialed {target}");
    handle_connection(stream, target, opts, state, false).await
}

async fn handle_connection(
    stream: TcpStream,
    remote_label: String,
    opts: PeerOptions,
    state: Shared,
    inbound: bool,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let (reader, writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Message>(64);

    let write_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(msg) = rx.recv().await {
            if write_msg(&mut writer, &msg).await.is_err() {
                break;
            }
        }
    });

    tx.send(Message::hello(
        &opts.node_id,
        &opts.name,
        &opts.listen,
        &opts.class,
        opts.score,
    ))
    .await
    .ok();

    {
        let addrs: Vec<String> = state.lock().known_addrs.iter().cloned().collect();
        if !addrs.is_empty() {
            tx.send(Message::Peers { addrs }).await.ok();
        }
    }

    let pending_pings: Arc<Mutex<HashMap<u64, Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let tx_ping = tx.clone();
    let pending_w = pending_pings.clone();
    let ping_task = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut nonce = 1u64;
        loop {
            tick.tick().await;
            pending_w.lock().insert(nonce, Instant::now());
            let msg = Message::Ping {
                nonce,
                ts_ms: Utc::now().timestamp_millis(),
            };
            if tx_ping.send(msg).await.is_err() {
                break;
            }
            nonce = nonce.wrapping_add(1);
        }
    });

    let mut reader = reader;
    let dir = if inbound { "←" } else { "→" };
    let mut peer_key = normalize_addr(&remote_label);

    loop {
        let msg = match read_msg(&mut reader).await {
            Ok(m) => m,
            Err(e) => {
                debug!("read {peer_key}: {e}");
                break;
            }
        };

        match msg {
            Message::Hello {
                protocol,
                node_id,
                name,
                listen,
                class,
                score,
            } => {
                if protocol != super::protocol::PROTOCOL {
                    warn!("peer protocol mismatch: {protocol}");
                }
                // Don't talk to ourselves
                if node_id == opts.node_id {
                    println!("[p2p] ignored self-connection");
                    break;
                }
                // Genesis ban enforcement
                {
                    let s = state.lock();
                    if let Some(reason) = s.banned.get(&node_id) {
                        println!(
                            "[p2p] REJECT banned peer {name} ({node_id}) reason={reason}"
                        );
                        break;
                    }
                }
                println!(
                    "[p2p] {dir} hello {name} ({node_id}) class={class} score={score:.0} listen={listen}"
                );
                peer_key = if listen.is_empty() {
                    normalize_addr(&remote_label)
                } else {
                    normalize_addr(&listen)
                };
                {
                    let mut s = state.lock();
                    if !listen.is_empty() {
                        s.known_addrs.insert(normalize_addr(&listen));
                    }
                    s.peers.insert(
                        peer_key.clone(),
                        PeerMeta {
                            node_id,
                            name,
                            listen: peer_key.clone(),
                            class,
                            score,
                            rtt_ms: None,
                        },
                    );
                }
                let addrs: Vec<String> = state.lock().known_addrs.iter().cloned().collect();
                tx.send(Message::Peers { addrs }).await.ok();
            }
            Message::Ping { nonce, ts_ms } => {
                // Echo original ts so peer measures RTT
                tx.send(Message::Pong {
                    nonce,
                    echo_ts_ms: ts_ms,
                })
                .await
                .ok();
            }
            Message::Pong {
                nonce,
                echo_ts_ms: _,
            } => {
                let rtt = pending_pings
                    .lock()
                    .remove(&nonce)
                    .map(|t| t.elapsed().as_secs_f64() * 1000.0);
                if let Some(rtt) = rtt {
                    let mut s = state.lock();
                    if let Some(p) = s.peers.get_mut(&peer_key) {
                        p.rtt_ms = Some(rtt);
                        println!("[p2p] pong from {} rtt={:.2} ms", p.name, rtt);
                    }
                }
            }
            Message::Peers { addrs } => {
                let mut s = state.lock();
                let mut new = 0usize;
                for a in addrs {
                    let a = normalize_addr(&a);
                    if !addrs_equal(&a, &opts.listen) && s.known_addrs.insert(a) {
                        new += 1;
                    }
                }
                if new > 0 {
                    println!("[p2p] learned {new} new peer address(es)");
                }
            }
        }
    }

    {
        let mut s = state.lock();
        if s.peers.remove(&peer_key).is_some() {
            println!("[p2p] disconnected {peer_key}");
        }
    }

    ping_task.abort();
    write_task.abort();
    Ok(())
}

async fn write_msg<W: AsyncWriteExt + Unpin>(w: &mut W, msg: &Message) -> Result<()> {
    let bytes = serde_json::to_vec(msg)?;
    if bytes.len() > 1_000_000 {
        return Err(anyhow!("message too large"));
    }
    let len = (bytes.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

async fn read_msg<R: AsyncReadExt + Unpin>(r: &mut R) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > 1_000_000 {
        return Err(anyhow!("bad frame len {len}"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}

fn normalize_addr(a: &str) -> String {
    a.trim().trim_start_matches("tcp://").to_string()
}

fn addrs_equal(a: &str, b: &str) -> bool {
    normalize_addr(a) == normalize_addr(b)
}

async fn refresh_truth(
    url: &str,
    expected_pubkey: Option<&str>,
    state: &Shared,
) -> Result<()> {
    let truth =
        crate::genesis::store::fetch_truth(url, expected_pubkey).await?;
    let mut s = state.lock();
    if truth.body.epoch < s.truth_epoch {
        // ignore older snapshots (replay)
        return Ok(());
    }
    if truth.body.epoch > s.truth_epoch {
        println!(
            "[p2p] genesis truth epoch {} → {} ({} bans, {} tracked)",
            s.truth_epoch,
            truth.body.epoch,
            crate::genesis::ban_count(&truth),
            truth.body.tracked.len()
        );
    }
    s.truth_epoch = truth.body.epoch;
    s.banned.clear();
    for b in &truth.body.banned {
        s.banned.insert(b.peer_id.clone(), b.reason.clone());
    }
    // drop active connections to newly banned peers
    s.peers.retain(|_, p| {
        if crate::genesis::is_banned(&truth, &p.node_id) {
            println!("[p2p] dropping banned peer {}", p.node_id);
            false
        } else {
            true
        }
    });
    Ok(())
}
