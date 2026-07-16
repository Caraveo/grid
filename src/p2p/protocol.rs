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
}

impl Message {
    pub fn hello(node_id: &str, name: &str, listen: &str, class: &str, score: f64) -> Self {
        Self::Hello {
            protocol: PROTOCOL.into(),
            node_id: node_id.into(),
            name: name.into(),
            listen: listen.into(),
            class: class.into(),
            score,
        }
    }
}
