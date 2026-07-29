use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "grid-p2p/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello {
        protocol: String,
        node_id: String,
        name: String,
        listen: String,
        class: String,
        #[serde(default)]
        score: f64,
        /// 128-hex GP id (optional for legacy peers)
        #[serde(default)]
        gp_id: Option<String>,
        /// Realm label (optional)
        #[serde(default)]
        realm: Option<String>,
        /// Operator pubkey hex (optional)
        #[serde(default)]
        pubkey_hex: Option<String>,
    },
    Ping {
        nonce: u64,
        /// Sender wall time (ms) — echoed in Pong for RTT.
        ts_ms: i64,
    },
    Pong {
        nonce: u64,
        /// Echo of Ping.ts_ms so sender can compute RTT.
        echo_ts_ms: i64,
    },
    /// Share known listen addresses so the mesh can grow.
    Peers {
        addrs: Vec<String>,
    },
    /// Request dial info for a GP id or realm.
    Find {
        /// Query nonce for matching Found
        nonce: u64,
        #[serde(default)]
        gp_id: Option<String>,
        #[serde(default)]
        realm: Option<String>,
    },
    /// Response to Find (may be multi-valued via multiple Found messages).
    Found {
        nonce: u64,
        #[serde(default)]
        gp_id: Option<String>,
        #[serde(default)]
        realm: Option<String>,
        node_id: String,
        name: String,
        listen: String,
    },
    GetBlocks {
        from_height: u64,
    },
    Blocks {
        blocks: Vec<crate::blockchain::Block>,
    },
    /// Request a private Engine service stream. This is deliberately a P2P
    /// control message, never a public URL or a host-port forwarding request.
    TunnelOpen {
        service: String,
        capability: String,
        client_pubkey: String,
        client_signature: String,
        request_id: String,
    },
    /// The host either accepts a capability-gated stream or fails closed.
    TunnelResult {
        request_id: String,
        accepted: bool,
        reason: Option<String>,
    },
    /// Encrypted tunnel payload carried inside the existing Noise session.
    TunnelData {
        request_id: String,
        data: Vec<u8>,
    },
    TunnelClose {
        request_id: String,
    },
}

impl Message {
    pub fn hello(
        node_id: &str,
        name: &str,
        listen: &str,
        class: &str,
        score: f64,
        gp_id: Option<String>,
        realm: Option<String>,
        pubkey_hex: Option<String>,
    ) -> Self {
        Self::Hello {
            protocol: PROTOCOL.into(),
            node_id: node_id.into(),
            name: name.into(),
            listen: listen.into(),
            class: class.into(),
            score,
            gp_id,
            realm,
            pubkey_hex,
        }
    }
}
