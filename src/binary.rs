use base64::Engine;
/// Binary frame codec for WebSocket binary messages.
///
/// Frame layout: `[1B type][8B id (big-endian)][payload…]`
use base64::engine::general_purpose::STANDARD as B64;

use crate::protocol::{Event, Request};

pub mod msg_type {
    // Requests (client → server)
    pub const CONNECT: u8 = 0x01;
    pub const BIND: u8 = 0x02;
    pub const LISTEN: u8 = 0x03;
    pub const SEND: u8 = 0x04;
    pub const CLOSE: u8 = 0x05;
    pub const CONNECT_UDP: u8 = 0x06;
    pub const SEND_TO: u8 = 0x07;
    pub const RESOLVE: u8 = 0x08;
    pub const CONNECT_TLS: u8 = 0x09;

    // Events (server → client)
    pub const CONNECTED: u8 = 0x81;
    pub const DATA: u8 = 0x82;
    pub const LISTENING: u8 = 0x83;
    pub const ACCEPTED: u8 = 0x84;
    pub const CLOSED: u8 = 0x85;
    pub const ERROR: u8 = 0x86;
    pub const DENIED: u8 = 0x87;
    pub const DATA_FROM: u8 = 0x88;
    pub const RESOLVED: u8 = 0x89;
    pub const UDP_BOUND: u8 = 0x8A;
}

pub fn decode_request(data: &[u8]) -> Result<Request, String> {
    if data.len() < 9 {
        return Err("binary frame too short".into());
    }
    let mt = data[0];
    let id = u64::from_be_bytes(data[1..9].try_into().unwrap());
    let payload = &data[9..];

    match mt {
        msg_type::CONNECT | msg_type::BIND | msg_type::CONNECT_UDP | msg_type::CONNECT_TLS => {
            if payload.len() < 2 {
                return Err("frame missing port".into());
            }
            let port = u16::from_be_bytes(payload[0..2].try_into().unwrap());
            let addr =
                String::from_utf8(payload[2..].to_vec()).map_err(|e| format!("bad addr: {e}"))?;
            match mt {
                msg_type::CONNECT => Ok(Request::Connect { id, addr, port }),
                msg_type::BIND => Ok(Request::Bind { id, addr, port }),
                msg_type::CONNECT_UDP => Ok(Request::ConnectUdp { id, addr, port }),
                msg_type::CONNECT_TLS => Ok(Request::ConnectTls { id, addr, port }),
                _ => unreachable!(),
            }
        }
        msg_type::LISTEN => {
            let backlog = if payload.len() >= 4 {
                u32::from_be_bytes(payload[0..4].try_into().unwrap())
            } else {
                128
            };
            Ok(Request::Listen { id, backlog })
        }
        msg_type::SEND => {
            let encoded = B64.encode(payload);
            Ok(Request::Send { id, data: encoded })
        }
        msg_type::CLOSE => Ok(Request::Close { id }),
        msg_type::SEND_TO => {
            if payload.len() < 4 {
                return Err("send_to frame too short".into());
            }
            let port = u16::from_be_bytes(payload[0..2].try_into().unwrap());
            let addr_len = u16::from_be_bytes(payload[2..4].try_into().unwrap()) as usize;
            if payload.len() < 4 + addr_len {
                return Err("send_to addr truncated".into());
            }
            let addr = String::from_utf8(payload[4..4 + addr_len].to_vec())
                .map_err(|e| format!("bad addr: {e}"))?;
            let data = B64.encode(&payload[4 + addr_len..]);
            Ok(Request::SendTo {
                id,
                addr,
                port,
                data,
            })
        }
        msg_type::RESOLVE => {
            let name = String::from_utf8(payload.to_vec()).map_err(|e| format!("bad name: {e}"))?;
            Ok(Request::Resolve { id, name })
        }
        _ => Err(format!("unknown message type: 0x{mt:02x}")),
    }
}

pub fn encode_event(event: &Event) -> Vec<u8> {
    match event {
        Event::Connected { id } => frame(msg_type::CONNECTED, *id, &[]),
        Event::Data { id, data } => {
            let raw = B64.decode(data).unwrap_or_default();
            frame(msg_type::DATA, *id, &raw)
        }
        Event::Listening { id, port } => frame(msg_type::LISTENING, *id, &port.to_be_bytes()),
        Event::Accepted {
            id,
            conn_id,
            remote,
        } => {
            let mut p = conn_id.to_be_bytes().to_vec();
            p.extend_from_slice(remote.as_bytes());
            frame(msg_type::ACCEPTED, *id, &p)
        }
        Event::Closed { id } => frame(msg_type::CLOSED, *id, &[]),
        Event::Error { id, msg } => frame(msg_type::ERROR, *id, msg.as_bytes()),
        Event::Denied { id, msg } => frame(msg_type::DENIED, *id, msg.as_bytes()),
        Event::DataFrom {
            id,
            data,
            addr,
            port,
        } => {
            let raw = B64.decode(data).unwrap_or_default();
            let ab = addr.as_bytes();
            let mut p = Vec::with_capacity(4 + ab.len() + raw.len());
            p.extend_from_slice(&port.to_be_bytes());
            p.extend_from_slice(&(ab.len() as u16).to_be_bytes());
            p.extend_from_slice(ab);
            p.extend_from_slice(&raw);
            frame(msg_type::DATA_FROM, *id, &p)
        }
        Event::Resolved { id, addrs } => {
            let json = serde_json::to_vec(addrs).unwrap_or_default();
            frame(msg_type::RESOLVED, *id, &json)
        }
        Event::UdpBound { id, port } => frame(msg_type::UDP_BOUND, *id, &port.to_be_bytes()),
    }
}

fn frame(mt: u8, id: u64, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9 + payload.len());
    buf.push(mt);
    buf.extend_from_slice(&id.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_connect() {
        let req = Request::Connect {
            id: 42,
            addr: "example.com".into(),
            port: 443,
        };
        let mut data = vec![msg_type::CONNECT];
        data.extend_from_slice(&42u64.to_be_bytes());
        data.extend_from_slice(&443u16.to_be_bytes());
        data.extend_from_slice(b"example.com");

        let decoded = decode_request(&data).unwrap();
        assert_eq!(decoded.id(), req.id());
    }

    #[test]
    fn roundtrip_data_event() {
        let raw = b"hello world";
        let ev = Event::Data {
            id: 7,
            data: B64.encode(raw),
        };
        let encoded = encode_event(&ev);
        assert_eq!(encoded[0], msg_type::DATA);
        let id = u64::from_be_bytes(encoded[1..9].try_into().unwrap());
        assert_eq!(id, 7);
        assert_eq!(&encoded[9..], raw);
    }

    #[test]
    fn send_carries_raw_bytes() {
        let raw = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let mut data = vec![msg_type::SEND];
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&raw);

        let decoded = decode_request(&data).unwrap();
        if let Request::Send { data, .. } = decoded {
            assert_eq!(B64.decode(&data).unwrap(), raw);
        } else {
            panic!("expected Send");
        }
    }

    #[test]
    fn too_short_rejected() {
        assert!(decode_request(&[0x01, 0, 0, 0]).is_err());
    }
}
