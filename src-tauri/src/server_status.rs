//! Minecraft Server List Ping — the live player count shown on the landing
//! view. Port of what `getServerStatus` provided in helios-core.
//!
//! Implements the modern (1.7+) handshake protocol:
//!   -> Handshake  { protocol, address, port, next_state = 1 }
//!   -> Status request
//!   <- JSON { version, players: { online, max }, description }
//!
//! Every field is length-prefixed with a varint, which is the fiddly part and
//! where the tests below are aimed.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{Error, Result};

/// Protocol version sent in the handshake. Any value works for a status ping
/// — servers answer regardless — so this matches what the JS used.
const PROTOCOL_VERSION: i32 = 47;

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if v == 0 {
            break;
        }
    }
    let _ = &mut value;
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

/// Read a varint one byte at a time; the length is not known up front.
async fn read_varint(stream: &mut TcpStream) -> Result<i32> {
    let mut result: i32 = 0;
    for shift in 0..5 {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b).await?;
        result |= ((b[0] & 0x7F) as i32) << (7 * shift);
        if b[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(Error::Other("Malformed varint from server".into()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusPlayers {
    pub online: i64,
    pub max: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusVersion {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub protocol: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawStatus {
    #[serde(default)]
    pub version: Option<StatusVersion>,
    #[serde(default)]
    pub players: Option<StatusPlayers>,
}

/// What the frontend needs. `online` is None when the server is unreachable,
/// which the UI renders the way the Electron build did — as an offline label
/// rather than a zero count, since those mean different things.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub online: bool,
    pub players_online: Option<i64>,
    pub players_max: Option<i64>,
    pub version: Option<String>,
}

impl ServerStatus {
    fn offline() -> Self {
        Self {
            online: false,
            players_online: None,
            players_max: None,
            version: None,
        }
    }
}

/// Ping a server. Never errors on an unreachable host — an offline server is
/// an expected state, not a failure, and the landing view must still render.
pub async fn ping(host: &str, port: u16) -> ServerStatus {
    match tokio::time::timeout(Duration::from_secs(5), ping_inner(host, port)).await {
        Ok(Ok(s)) => s,
        Ok(Err(err)) => {
            tracing::debug!(%host, port, %err, "Server ping failed");
            ServerStatus::offline()
        }
        Err(_) => {
            tracing::debug!(%host, port, "Server ping timed out");
            ServerStatus::offline()
        }
    }
}

async fn ping_inner(host: &str, port: u16) -> Result<ServerStatus> {
    let mut stream = TcpStream::connect((host, port)).await?;

    // Handshake, wrapped in its own length prefix.
    let mut body = Vec::new();
    write_varint(&mut body, 0x00);
    write_varint(&mut body, PROTOCOL_VERSION);
    write_string(&mut body, host);
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, 1); // next state: status

    let mut packet = Vec::new();
    write_varint(&mut packet, body.len() as i32);
    packet.extend_from_slice(&body);
    stream.write_all(&packet).await?;

    // Status request: a single empty packet.
    stream.write_all(&[0x01, 0x00]).await?;
    stream.flush().await?;

    let _len = read_varint(&mut stream).await?;
    let packet_id = read_varint(&mut stream).await?;
    if packet_id != 0x00 {
        return Err(Error::Other(format!(
            "Unexpected status packet id {packet_id}"
        )));
    }

    let json_len = read_varint(&mut stream).await?;
    if json_len <= 0 || json_len > 1_048_576 {
        return Err(Error::Other("Implausible status payload length".into()));
    }
    let mut buf = vec![0u8; json_len as usize];
    stream.read_exact(&mut buf).await?;

    let raw: RawStatus = serde_json::from_slice(&buf)?;
    Ok(ServerStatus {
        online: true,
        players_online: raw.players.as_ref().map(|p| p.online),
        players_max: raw.players.as_ref().map(|p| p.max),
        version: raw.version.and_then(|v| v.name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(v: i32) -> Vec<u8> {
        let mut b = Vec::new();
        write_varint(&mut b, v);
        b
    }

    #[test]
    fn varint_encoding_matches_the_protocol_spec() {
        // Values from the documented test vectors.
        assert_eq!(varint(0), vec![0x00]);
        assert_eq!(varint(1), vec![0x01]);
        assert_eq!(varint(127), vec![0x7F]);
        assert_eq!(varint(128), vec![0x80, 0x01]);
        assert_eq!(varint(255), vec![0xFF, 0x01]);
        assert_eq!(varint(25565), vec![0xDD, 0xC7, 0x01]);
        assert_eq!(varint(2097151), vec![0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn strings_are_length_prefixed() {
        let mut b = Vec::new();
        write_string(&mut b, "mc.example.com");
        assert_eq!(b[0], 14);
        assert_eq!(&b[1..], b"mc.example.com");
    }

    #[test]
    fn status_json_parses_the_shape_servers_send() {
        let raw = r#"{
            "version": { "name": "1.20.1", "protocol": 763 },
            "players": { "max": 100, "online": 22, "sample": [] },
            "description": { "text": "A Minecraft Server" }
        }"#;
        let s: RawStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(s.players.as_ref().unwrap().online, 22);
        assert_eq!(s.players.as_ref().unwrap().max, 100);
        assert_eq!(s.version.unwrap().name.unwrap(), "1.20.1");
    }

    #[test]
    fn missing_players_block_is_tolerated() {
        // Some proxies omit it; this must not fail the whole ping.
        let s: RawStatus = serde_json::from_str(r#"{"description":"hi"}"#).unwrap();
        assert!(s.players.is_none());
    }

    #[tokio::test]
    async fn unreachable_host_reports_offline_rather_than_erroring() {
        // Reserved TEST-NET-1 address; nothing answers.
        let s = ping("192.0.2.1", 25565).await;
        assert!(!s.online);
        assert!(s.players_online.is_none());
    }

    /// Pings a real public server. Ignored by default (network).
    #[tokio::test]
    #[ignore]
    async fn pings_a_live_server() {
        let s = ping("mc.hypixel.net", 25565).await;
        println!("{s:?}");
        assert!(s.online, "expected hypixel to answer");
        assert!(s.players_online.unwrap_or(0) > 0);
    }
}
