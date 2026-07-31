//! TCP length-prefixed JSON P2P (minimal MVP).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
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
    /// 128-hex GP id for this peer (optional).
    pub gp_id: Option<String>,
    /// Realm this peer serves (optional).
    pub realm: Option<String>,
    /// Operator pubkey hex (optional).
    pub pubkey_hex: Option<String>,
    /// In-memory static key derived after the operator passkey vault is unlocked.
    pub noise_static_key: [u8; 32],
    pub config_dir: PathBuf,
}

#[derive(Clone)]
struct PeerMeta {
    node_id: String,
    name: String,
    listen: String,
    class: String,
    score: f64,
    rtt_ms: Option<f64>,
    gp_id: Option<String>,
    realm: Option<String>,
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
                if let Err(e) = refresh_truth(&gurl, gpk.as_deref(), &state_t).await {
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
    if let Some(ref g) = opts.gp_id {
        println!("  gp_id    {}…", &g[..g.len().min(16)]);
    }
    if let Some(ref r) = opts.realm {
        println!("  realm    grid://{r}.grid");
    }
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
                        let realm = p
                            .realm
                            .as_deref()
                            .map(|r| format!(" grid://{r}.grid"))
                            .unwrap_or_default();
                        println!(
                            "       · {} ({}) class={} score={:.0} rtt={} @ {}{realm}",
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
            if let Err(e) = handle_connection(stream, remote.to_string(), opts, state, true).await {
                debug!("inbound {remote}: {e}");
            }
        });
    }
}

/// Open one private, capability-gated TCP stream through an existing GRID
/// Noise protocol session. The local listener is loopback-only and handles one
/// client connection; no public socket or DNS entry is created.
pub async fn run_private_tunnel_client(
    opts: PeerOptions,
    peer: &str,
    service: &str,
    capability: &str,
    client_pubkey: &str,
    client_signature: &str,
    local_bind: &str,
) -> Result<()> {
    let bind: SocketAddr = local_bind
        .parse()
        .with_context(|| format!("bad private tunnel bind {local_bind}"))?;
    if !bind.ip().is_loopback() {
        return Err(anyhow!("private tunnel client must bind to loopback"));
    }
    let listener = TcpListener::bind(bind).await?;
    println!("GRID private tunnel listening on http://{local_bind}");
    println!("  service  grid://service/{service}");
    println!("  peer     {peer}");
    println!("  accepts  one local connection per capability");
    let (local, _) = listener.accept().await?;

    let stream = TcpStream::connect(normalize_addr(peer))
        .await
        .with_context(|| format!("connect private tunnel peer {peer}"))?;
    let (stream, transport) = noise_handshake(stream, false, &opts.noise_static_key).await?;
    let transport = Arc::new(tokio::sync::Mutex::new(transport));
    let (mut remote_reader, mut remote_writer) = stream.into_split();
    write_msg(
        &mut remote_writer,
        &transport,
        &Message::hello(
            &opts.node_id,
            &opts.name,
            "",
            &opts.class,
            opts.score,
            None,
            None,
            opts.pubkey_hex.clone(),
        ),
    )
    .await?;
    let request_id = uuid::Uuid::new_v4().to_string();
    write_msg(
        &mut remote_writer,
        &transport,
        &Message::TunnelOpen {
            service: service.into(),
            capability: capability.into(),
            client_pubkey: client_pubkey.into(),
            client_signature: client_signature.into(),
            request_id: request_id.clone(),
        },
    )
    .await?;
    loop {
        match read_msg(&mut remote_reader, &transport).await? {
            Message::TunnelResult {
                request_id: id,
                accepted,
                reason,
            } if id == request_id => {
                if !accepted {
                    bail!(
                        "private tunnel refused: {}",
                        reason.unwrap_or_else(|| "policy".into())
                    );
                }
                break;
            }
            Message::Ping { nonce, ts_ms } => {
                write_msg(
                    &mut remote_writer,
                    &transport,
                    &Message::Pong {
                        nonce,
                        echo_ts_ms: ts_ms,
                    },
                )
                .await?;
            }
            _ => {}
        }
    }

    let (mut local_reader, mut local_writer) = local.into_split();
    let mut local_buffer = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = local_reader.read(&mut local_buffer) => {
                let size = read?;
                if size == 0 {
                    write_msg(
                        &mut remote_writer,
                        &transport,
                        &Message::TunnelClose { request_id: request_id.clone() },
                    ).await?;
                    break;
                }
                write_msg(
                    &mut remote_writer,
                    &transport,
                    &Message::TunnelData {
                        request_id: request_id.clone(),
                        data: local_buffer[..size].to_vec(),
                    },
                ).await?;
            }
            message = read_msg(&mut remote_reader, &transport) => {
                match message? {
                    Message::TunnelData { request_id: id, data } if id == request_id => {
                        local_writer.write_all(&data).await?;
                    }
                    Message::TunnelClose { request_id: id } if id == request_id => break,
                    Message::Ping { nonce, ts_ms } => {
                        write_msg(
                            &mut remote_writer,
                            &transport,
                            &Message::Pong { nonce, echo_ts_ms: ts_ms },
                        ).await?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
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
    let (stream, transport) = noise_handshake(stream, inbound, &opts.noise_static_key).await?;
    let transport = Arc::new(tokio::sync::Mutex::new(transport));
    let (reader, writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Message>(64);
    let transport_writer = transport.clone();

    let write_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(msg) = rx.recv().await {
            if write_msg(&mut writer, &transport_writer, &msg)
                .await
                .is_err()
            {
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
        opts.gp_id.clone(),
        opts.realm.clone(),
        opts.pubkey_hex.clone(),
    ))
    .await
    .ok();
    tx.send(Message::GetBlocks { from_height: 0 }).await.ok();

    {
        let addrs: Vec<String> = state.lock().known_addrs.iter().cloned().collect();
        if !addrs.is_empty() {
            tx.send(Message::Peers { addrs }).await.ok();
        }
    }

    let pending_pings: Arc<Mutex<HashMap<u64, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

    let tx_ping = tx.clone();
    let pending_w = pending_pings.clone();
    let chain_dir = opts.config_dir.clone();
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
            let from_height = crate::blockchain::ChainReplica::load(&chain_dir)
                .ok()
                .flatten()
                .map(|replica| replica.tip().height.saturating_add(1))
                .unwrap_or(0);
            if tx_ping
                .send(Message::GetBlocks { from_height })
                .await
                .is_err()
            {
                break;
            }
            nonce = nonce.wrapping_add(1);
        }
    });

    let mut reader = reader;
    let dir = if inbound { "←" } else { "→" };
    let mut peer_key = normalize_addr(&remote_label);
    let mut tunnel_writers: HashMap<String, tokio::net::tcp::OwnedWriteHalf> = HashMap::new();

    loop {
        let msg = match read_msg(&mut reader, &transport).await {
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
                gp_id,
                realm,
                pubkey_hex: _,
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
                        println!("[p2p] REJECT banned peer {name} ({node_id}) reason={reason}");
                        break;
                    }
                }
                let realm_s = realm
                    .as_deref()
                    .map(|r| format!(" realm=grid://{r}.grid"))
                    .unwrap_or_default();
                println!(
                    "[p2p] {dir} hello {name} ({node_id}) class={class} score={score:.0} listen={listen}{realm_s}"
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
                            gp_id,
                            realm,
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
            Message::TunnelOpen {
                service,
                capability,
                client_pubkey,
                client_signature,
                request_id,
            } => {
                let runtime = crate::engine::service_status(&opts.config_dir, &service).ok();
                let eligible = runtime.as_ref().is_some_and(|state| {
                    state.state == "running-private" && !state.public_exposure
                });
                if !eligible {
                    tx.send(Message::TunnelResult {
                        request_id,
                        accepted: false,
                        reason: Some("private Engine service is not running".into()),
                    })
                    .await
                    .ok();
                    continue;
                }
                let runtime = runtime.expect("eligible runtime");
                let local = TcpStream::connect(("127.0.0.1", runtime.loopback_port)).await;
                if local.is_err()
                    || !crate::engine::consume_private_capability(
                        &opts.config_dir,
                        &service,
                        &capability,
                        &client_pubkey,
                        &client_signature,
                    )
                {
                    tx.send(Message::TunnelResult {
                        request_id,
                        accepted: false,
                        reason: Some("invalid capability or unavailable service".into()),
                    })
                    .await
                    .ok();
                    continue;
                }
                let (mut local_reader, local_writer) =
                    local.expect("checked local stream").into_split();
                tunnel_writers.insert(request_id.clone(), local_writer);
                tx.send(Message::TunnelResult {
                    request_id: request_id.clone(),
                    accepted: true,
                    reason: None,
                })
                .await
                .ok();
                let tunnel_tx = tx.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16 * 1024];
                    loop {
                        match local_reader.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(size) => {
                                if tunnel_tx
                                    .send(Message::TunnelData {
                                        request_id: request_id.clone(),
                                        data: buffer[..size].to_vec(),
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    let _ = tunnel_tx.send(Message::TunnelClose { request_id }).await;
                });
            }
            Message::TunnelResult {
                request_id,
                accepted,
                reason,
            } => {
                if !accepted {
                    debug!(
                        "private tunnel {request_id} refused: {}",
                        reason.unwrap_or_else(|| "policy".into())
                    );
                }
            }
            Message::TunnelData { request_id, data } => {
                if data.len() > 16 * 1024 {
                    warn!("private tunnel frame too large");
                    tunnel_writers.remove(&request_id);
                } else if let Some(writer) = tunnel_writers.get_mut(&request_id) {
                    if writer.write_all(&data).await.is_err() {
                        tunnel_writers.remove(&request_id);
                    }
                } else {
                    debug!("ignored tunnel data for unknown request {request_id}");
                }
            }
            Message::TunnelClose { request_id } => {
                tunnel_writers.remove(&request_id);
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
            Message::Find {
                nonce,
                gp_id,
                realm,
            } => {
                // Answer from local knowledge (self + peers)
                let mut hits: Vec<(String, String, String, Option<String>, Option<String>)> =
                    Vec::new();
                // Self match
                let self_match = match (&gp_id, &realm, &opts.gp_id, &opts.realm) {
                    (Some(q), _, Some(mine), _) if q == mine => true,
                    (_, Some(q), _, Some(mine)) if q.eq_ignore_ascii_case(mine) => true,
                    _ => false,
                };
                if self_match {
                    hits.push((
                        opts.node_id.clone(),
                        opts.name.clone(),
                        opts.listen.clone(),
                        opts.gp_id.clone(),
                        opts.realm.clone(),
                    ));
                }
                {
                    let s = state.lock();
                    for p in s.peers.values() {
                        let ok = match (&gp_id, &realm) {
                            (Some(q), _) => p.gp_id.as_ref() == Some(q),
                            (_, Some(q)) => p
                                .realm
                                .as_ref()
                                .map(|r| r.eq_ignore_ascii_case(q))
                                .unwrap_or(false),
                            _ => false,
                        };
                        if ok {
                            hits.push((
                                p.node_id.clone(),
                                p.name.clone(),
                                p.listen.clone(),
                                p.gp_id.clone(),
                                p.realm.clone(),
                            ));
                        }
                    }
                }
                for (node_id, name, listen, g, r) in hits {
                    tx.send(Message::Found {
                        nonce,
                        gp_id: g,
                        realm: r,
                        node_id,
                        name,
                        listen,
                    })
                    .await
                    .ok();
                }
            }
            Message::Found {
                nonce,
                gp_id,
                realm,
                node_id,
                name,
                listen,
            } => {
                let a = normalize_addr(&listen);
                {
                    let mut s = state.lock();
                    s.known_addrs.insert(a.clone());
                    s.peers.entry(a.clone()).or_insert(PeerMeta {
                        node_id: node_id.clone(),
                        name: name.clone(),
                        listen: a.clone(),
                        class: "S".into(),
                        score: 0.0,
                        rtt_ms: None,
                        gp_id: gp_id.clone(),
                        realm: realm.clone(),
                    });
                }
                let r = realm
                    .as_deref()
                    .map(|x| format!("grid://{x}.grid"))
                    .unwrap_or_else(|| "—".into());
                println!("[p2p] found nonce={nonce} {name} ({node_id}) @ {a} · {r}");
            }
            Message::GetBlocks { from_height } => {
                if let Ok(Some(replica)) = crate::blockchain::ChainReplica::load(&opts.config_dir) {
                    // Bound every sync response well below the 1 MB encrypted
                    // protocol frame. The requester asks again from its new tip
                    // on the next tick, so large histories stream in chunks.
                    const MAX_SYNC_BLOCKS: usize = 25;
                    const MAX_SYNC_BYTES: usize = 700_000;
                    let mut blocks = Vec::new();
                    let mut bytes = 0usize;
                    for block in replica
                        .blocks
                        .into_iter()
                        .filter(|block| block.height >= from_height)
                    {
                        let block_bytes = serde_json::to_vec(&block)
                            .map(|encoded| encoded.len())
                            .unwrap_or(MAX_SYNC_BYTES);
                        if !blocks.is_empty()
                            && (blocks.len() >= MAX_SYNC_BLOCKS
                                || bytes.saturating_add(block_bytes) > MAX_SYNC_BYTES)
                        {
                            break;
                        }
                        bytes = bytes.saturating_add(block_bytes);
                        blocks.push(block);
                    }
                    tx.send(Message::Blocks { blocks }).await.ok();
                }
            }
            Message::Blocks { blocks } => {
                if blocks.is_empty() {
                    continue;
                }
                let mut replica = match crate::blockchain::ChainReplica::load(&opts.config_dir) {
                    Ok(Some(r)) => r,
                    Ok(None) => {
                        let Some(genesis) = blocks.iter().find(|b| b.height == 0).cloned() else {
                            continue;
                        };
                        if let Some(expected) = opts.genesis_pubkey.as_deref() {
                            if genesis.leader_pubkey != expected {
                                warn!("rejected block genesis with an untrusted leader key");
                                continue;
                            }
                        }
                        let r = crate::blockchain::ChainReplica {
                            chain_id: genesis.chain_id.clone(),
                            leader_pubkey: genesis.leader_pubkey.clone(),
                            max_supply: crate::chain::MAX_SUPPLY,
                            recovery_pubkeys: vec![],
                            blocks: vec![genesis],
                        };
                        if r.verify().is_err() {
                            continue;
                        }
                        r
                    }
                    Err(_) => continue,
                };
                let mut accepted = 0usize;
                for block in blocks {
                    if block.height <= replica.tip().height {
                        continue;
                    }
                    if replica.apply_replica_block(block).is_ok() {
                        accepted += 1;
                    } else {
                        break;
                    }
                }
                if accepted > 0 && replica.save(&opts.config_dir).is_ok() {
                    println!(
                        "[chain] synced {accepted} verified block(s), height={}",
                        replica.tip().height
                    );
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

async fn write_msg<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    transport: &Arc<tokio::sync::Mutex<snow::TransportState>>,
    msg: &Message,
) -> Result<()> {
    let bytes = serde_json::to_vec(msg)?;
    if bytes.len() > 1_000_000 {
        return Err(anyhow!("message too large"));
    }
    let mut encrypted = vec![0u8; bytes.len() + 16];
    let n = transport
        .lock()
        .await
        .write_message(&bytes, &mut encrypted)
        .map_err(|e| anyhow!("Noise encrypt: {e}"))?;
    encrypted.truncate(n);
    let len = (encrypted.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(&encrypted).await?;
    w.flush().await?;
    Ok(())
}

async fn read_msg<R: AsyncReadExt + Unpin>(
    r: &mut R,
    transport: &Arc<tokio::sync::Mutex<snow::TransportState>>,
) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > 1_000_000 {
        return Err(anyhow!("bad frame len {len}"));
    }
    let mut encrypted = vec![0u8; len];
    r.read_exact(&mut encrypted).await?;
    let mut plaintext = vec![0u8; len];
    let n = transport
        .lock()
        .await
        .read_message(&encrypted, &mut plaintext)
        .map_err(|e| anyhow!("Noise decrypt: {e}"))?;
    plaintext.truncate(n);
    Ok(serde_json::from_slice(&plaintext)?)
}

/// Noise XX establishes an encrypted, forward-secret session before a single
/// GRID protocol message is exchanged. The registry only helps locate peers.
async fn noise_handshake(
    mut stream: TcpStream,
    inbound: bool,
    static_key: &[u8; 32],
) -> Result<(TcpStream, snow::TransportState)> {
    const NOISE_PROLOGUE: &[u8] = b"GRID-P2P/2";
    let params: snow::params::NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .map_err(|e| anyhow!("Noise parameters: {e}"))?;
    let builder = snow::Builder::new(params)
        .prologue(NOISE_PROLOGUE)
        .local_private_key(static_key);
    let mut state = if inbound {
        builder.build_responder()
    } else {
        builder.build_initiator()
    }
    .map_err(|e| anyhow!("Noise init: {e}"))?;

    let mut out = [0u8; 1024];
    let mut input = vec![0u8; 1024];
    if inbound {
        let n = read_noise_frame(&mut stream, &mut input).await?;
        state
            .read_message(&input[..n], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
        let n = state
            .write_message(&[], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
        write_noise_frame(&mut stream, &out[..n]).await?;
        let n = read_noise_frame(&mut stream, &mut input).await?;
        state
            .read_message(&input[..n], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
    } else {
        let n = state
            .write_message(&[], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
        write_noise_frame(&mut stream, &out[..n]).await?;
        let n = read_noise_frame(&mut stream, &mut input).await?;
        state
            .read_message(&input[..n], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
        let n = state
            .write_message(&[], &mut out)
            .map_err(|e| anyhow!("Noise handshake: {e}"))?;
        write_noise_frame(&mut stream, &out[..n]).await?;
    }
    let transport = state
        .into_transport_mode()
        .map_err(|e| anyhow!("Noise transport: {e}"))?;
    Ok((stream, transport))
}

async fn write_noise_frame(stream: &mut TcpStream, body: &[u8]) -> Result<()> {
    if body.is_empty() || body.len() > 1024 {
        return Err(anyhow!("bad Noise handshake frame"));
    }
    stream.write_all(&(body.len() as u16).to_be_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_noise_frame(stream: &mut TcpStream, out: &mut Vec<u8>) -> Result<usize> {
    let mut size = [0u8; 2];
    stream.read_exact(&mut size).await?;
    let n = u16::from_be_bytes(size) as usize;
    if n == 0 || n > 1024 {
        return Err(anyhow!("bad Noise handshake frame length"));
    }
    out.resize(n, 0);
    stream.read_exact(out).await?;
    Ok(n)
}

fn normalize_addr(a: &str) -> String {
    a.trim().trim_start_matches("tcp://").to_string()
}

fn addrs_equal(a: &str, b: &str) -> bool {
    normalize_addr(a) == normalize_addr(b)
}

async fn refresh_truth(url: &str, expected_pubkey: Option<&str>, state: &Shared) -> Result<()> {
    let truth = crate::genesis::store::fetch_truth(url, expected_pubkey).await?;
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

#[cfg(test)]
mod tunnel_tests {
    use super::*;

    #[tokio::test]
    async fn private_tunnel_client_rejects_non_loopback_bind() {
        let options = PeerOptions {
            node_id: "node-test".into(),
            name: "test".into(),
            class: "S".into(),
            listen: String::new(),
            connect: Vec::new(),
            score: 0.0,
            genesis_url: None,
            genesis_pubkey: None,
            gp_id: None,
            realm: None,
            pubkey_hex: None,
            noise_static_key: [1u8; 32],
            config_dir: PathBuf::new(),
        };
        let error = run_private_tunnel_client(
            options,
            "127.0.0.1:9",
            "site",
            &"00".repeat(32),
            &"11".repeat(32),
            &"22".repeat(64),
            "0.0.0.0:41784",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("must bind to loopback"));
    }
}
