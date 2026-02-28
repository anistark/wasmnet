use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Connect {
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
    Close {
        id: u64,
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
    Listening {
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
}

impl Request {
    pub fn id(&self) -> u64 {
        match self {
            Request::Connect { id, .. }
            | Request::Bind { id, .. }
            | Request::Listen { id, .. }
            | Request::Send { id, .. }
            | Request::Close { id, .. } => *id,
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
