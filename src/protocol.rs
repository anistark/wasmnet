use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Connect {
        id: u64,
        addr: String,
        port: u16,
    },
    ConnectTls {
        id: u64,
        addr: String,
        port: u16,
    },
    ConnectUdp {
        id: u64,
        addr: String,
        port: u16,
    },
    Bind {
        id: u64,
        addr: String,
        port: u16,
    },
    Listen {
        id: u64,
        #[serde(default = "default_backlog")]
        backlog: u32,
    },
    Send {
        id: u64,
        data: String,
    },
    SendTo {
        id: u64,
        addr: String,
        port: u16,
        data: String,
    },
    Close {
        id: u64,
    },
    Resolve {
        id: u64,
        name: String,
    },
}

fn default_backlog() -> u32 {
    128
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event {
    Connected {
        id: u64,
    },
    Data {
        id: u64,
        data: String,
    },
    DataFrom {
        id: u64,
        data: String,
        addr: String,
        port: u16,
    },
    Listening {
        id: u64,
        port: u16,
    },
    UdpBound {
        id: u64,
        port: u16,
    },
    Accepted {
        id: u64,
        conn_id: u64,
        remote: String,
    },
    Closed {
        id: u64,
    },
    Error {
        id: u64,
        msg: String,
    },
    Denied {
        id: u64,
        msg: String,
    },
    Resolved {
        id: u64,
        addrs: Vec<String>,
    },
}

impl Request {
    pub fn id(&self) -> u64 {
        match self {
            Request::Connect { id, .. }
            | Request::ConnectTls { id, .. }
            | Request::ConnectUdp { id, .. }
            | Request::Bind { id, .. }
            | Request::Listen { id, .. }
            | Request::Send { id, .. }
            | Request::SendTo { id, .. }
            | Request::Close { id, .. }
            | Request::Resolve { id, .. } => *id,
        }
    }
}

impl Event {
    pub fn error(id: u64, msg: impl Into<String>) -> Self {
        Event::Error {
            id,
            msg: msg.into(),
        }
    }

    pub fn denied(id: u64, msg: impl Into<String>) -> Self {
        Event::Denied {
            id,
            msg: msg.into(),
        }
    }
}
