//! A record of the bytes `handle_packet` answers `GameSpy4` query requests with, made before the
//! function was split into one piece per request type (`tests/golden/query.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`).
//!
//! Each scene hands the function the bytes of one UDP datagram from a real loopback client, with
//! a challenge-token table the scene set up, and reads what the client got back (nothing, or the
//! reply bytes as hex and text). A handshake reply carries a random token: the line shows its
//! shape and whether the server stored it for the sender instead of its digits.
//! Servers: an empty one, one that fills up with players in two worlds and plugins, a bad MOTD
//! (a NUL byte), and one with a custom MOTD and player limit.

use std::collections::HashMap;
use std::ffi::NulError;
use std::fmt::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pumpkin_util::GameMode;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::plugin::PluginMetadata;
use crate::plugin::api::Plugin;
use crate::plugin::loader::{PluginLoadFuture, PluginLoader, PluginUnloadFuture};
use crate::server::Server;

use super::handle_packet;

/// A token the table binds to the client's own address.
const TOKEN_OK: i32 = 0x0102_0304;
/// A negative token bound to the client's own address.
const TOKEN_NEGATIVE: i32 = -77;
/// A token bound to the client's IP with another port.
const TOKEN_OTHER_PORT: i32 = 0x1111;
/// A token bound to another IP with the client's port.
const TOKEN_OTHER_IP: i32 = 0x2222;
/// A token the table does not hold (expired or never given).
const TOKEN_UNKNOWN: i32 = 0x3333;

type Tokens = Arc<RwLock<HashMap<i32, SocketAddr>>>;

/// A plugin that does nothing; only its name counts.
struct Stub;

impl Plugin for Stub {}

/// Loads a [`Stub`] named after the file stem of the path.
struct StubLoader;

impl PluginLoader for StubLoader {
    fn load<'a>(&'a self, path: &'a Path) -> PluginLoadFuture<'a> {
        Box::pin(async move {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_owned();
            let metadata = PluginMetadata {
                name,
                version: "1".to_owned(),
                authors: Vec::new(),
                description: String::new(),
                dependencies: Vec::new(),
                permissions: Vec::new(),
            };
            Ok((
                Arc::new(Stub) as Arc<dyn Plugin>,
                metadata,
                Box::new(()) as Box<dyn std::any::Any + Send + Sync>,
            ))
        })
    }

    fn can_load(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "stub")
    }

    fn unload(&self, _data: Box<dyn std::any::Any + Send + Sync>) -> PluginUnloadFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn can_unload(&self) -> bool {
        true
    }
}

/// A server, a server-side socket and a loopback client that sends from its own address.
struct Wire {
    server: Arc<Server>,
    socket: Arc<UdpSocket>,
    client: UdpSocket,
    client_addr: SocketAddr,
    tokens: Tokens,
}

impl Wire {
    async fn new(server: Arc<Server>) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        let client = UdpSocket::bind("127.0.0.1:0").await?;
        let client_addr = client.local_addr()?;
        let mut other_port = client_addr;
        other_port.set_port(client_addr.port().wrapping_add(1));
        let mut other_ip = client_addr;
        other_ip.set_ip([127, 0, 0, 2].into());
        let tokens = HashMap::from([
            (TOKEN_OK, client_addr),
            (TOKEN_NEGATIVE, client_addr),
            (TOKEN_OTHER_PORT, other_port),
            (TOKEN_OTHER_IP, other_ip),
        ]);
        Ok(Self {
            server,
            socket,
            client,
            client_addr,
            tokens: Arc::new(RwLock::new(tokens)),
        })
    }

    /// Sends `bytes` through `handle_packet` as if the client had, bound to `bound`; one line.
    async fn ask(&self, name: &str, bytes: &[u8], bound: SocketAddr, out: &mut String) {
        let result = handle_packet(
            bytes.to_vec(),
            self.tokens.clone(),
            self.server.clone(),
            self.socket.clone(),
            self.client_addr,
            bound,
        )
        .await;
        let mut reply = vec![0u8; 2048];
        let got = tokio::time::timeout(
            Duration::from_millis(200),
            self.client.recv_from(&mut reply),
        )
        .await;
        let reply = match got {
            Ok(Ok((length, from))) if from == self.socket.local_addr().unwrap_or(from) => {
                reply.truncate(length);
                Some(reply)
            }
            _ => None,
        };
        let described = match &reply {
            None => "no reply".to_owned(),
            Some(reply) => self.describe(reply).await,
        };
        let _ = writeln!(
            out,
            "{name}: in [{}] -> {} | {described}",
            hex(bytes),
            outcome(&result)
        );
    }

    /// The reply bytes; a handshake reply as its shape plus what the token table now holds.
    async fn describe(&self, reply: &[u8]) -> String {
        if reply.first() != Some(&9) {
            return format!("[{}] {:?}", hex(reply), text(reply));
        }
        let digits = reply
            .get(5..reply.len().saturating_sub(1))
            .unwrap_or_default();
        let token = std::str::from_utf8(digits)
            .ok()
            .and_then(|digits| digits.parse::<i32>().ok());
        let tokens = self.tokens.read().await;
        let stored = token.and_then(|token| tokens.get(&token));
        format!(
            "handshake head [{}] token digits 1 to 10 {} positive {} ends with nul {} stored for sender {} tokens held {}",
            hex(reply.get(..5).unwrap_or_default()),
            (1..=10).contains(&digits.len()),
            token.is_some_and(|token| token > 0),
            reply.last() == Some(&0),
            stored == Some(&self.client_addr),
            tokens.len(),
        )
    }
}

fn outcome(result: &Result<(), NulError>) -> &'static str {
    match result {
        Ok(()) => "ok",
        Err(_) => "nul error",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The bytes with every non-printable one as `.`, with NUL as `~`.
fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0 => '~',
            0x20..=0x7e => char::from(*byte),
            _ => '.',
        })
        .collect()
}

fn handshake(session: i32) -> Vec<u8> {
    let mut bytes = vec![0xfe, 0xfd, 9];
    bytes.extend_from_slice(&session.to_be_bytes());
    bytes
}

fn status(session: i32, token: i32, padding: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xfe, 0xfd, 0];
    bytes.extend_from_slice(&session.to_be_bytes());
    bytes.extend_from_slice(&token.to_be_bytes());
    bytes.extend_from_slice(padding);
    bytes
}

fn addr(text: &str) -> SocketAddr {
    text.parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)))
}

/// Datagrams that are no query request: wrong magic, unknown type, cut short.
async fn malformed_scenes(wire: &Wire, bound: SocketAddr, out: &mut String) {
    let cases: [(&str, &[u8]); 14] = [
        ("empty", &[]),
        ("one byte", &[0xfe]),
        ("magic only", &[0xfe, 0xfd]),
        ("bad magic low", &[0xfe, 0xfc, 9, 0, 0, 0, 1]),
        ("bad magic high", &[0xff, 0xfd, 9, 0, 0, 0, 1]),
        ("bad magic zero", &[0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2]),
        ("type 1", &[0xfe, 0xfd, 1, 0, 0, 0, 1]),
        ("type 10", &[0xfe, 0xfd, 10, 0, 0, 0, 1]),
        ("type 255", &[0xfe, 0xfd, 255, 0, 0, 0, 1, 0, 0, 0, 2]),
        ("handshake no session", &[0xfe, 0xfd, 9]),
        ("handshake 3 session bytes", &[0xfe, 0xfd, 9, 0, 0, 1]),
        ("status no session", &[0xfe, 0xfd, 0]),
        ("status session only", &[0xfe, 0xfd, 0, 0, 0, 0, 1]),
        (
            "status 3 token bytes",
            &[0xfe, 0xfd, 0, 0, 0, 0, 1, 1, 2, 3],
        ),
    ];
    for (name, bytes) in cases {
        wire.ask(name, bytes, bound, out).await;
    }
}

/// Handshakes: the reply token, the token table, session ids and trailing bytes.
async fn handshake_scenes(wire: &Wire, bound: SocketAddr, out: &mut String) {
    for (name, session) in [
        ("handshake", 1),
        ("handshake again", 1),
        ("handshake session 0", 0),
        ("handshake session -1", -1),
        ("handshake session max", i32::MAX),
        ("handshake session min", i32::MIN),
    ] {
        wire.ask(name, &handshake(session), bound, out).await;
    }
    let mut trailing = handshake(0x0a0b_0c0d);
    trailing.extend_from_slice(&[1, 2, 3]);
    wire.ask("handshake trailing bytes", &trailing, bound, out)
        .await;
}

/// Status requests: the token check, the padding that tells basic from full, sessions.
async fn request_scenes(wire: &Wire, bound: SocketAddr, out: &mut String) {
    let session = 0x0000_0102;
    for (name, token) in [
        ("basic bound token", TOKEN_OK),
        ("basic negative token", TOKEN_NEGATIVE),
        ("basic unknown token", TOKEN_UNKNOWN),
        ("basic token zero", 0),
        ("basic other port", TOKEN_OTHER_PORT),
        ("basic other ip", TOKEN_OTHER_IP),
    ] {
        wire.ask(name, &status(session, token, &[]), bound, out)
            .await;
    }
    for (name, token) in [
        ("full bound token", TOKEN_OK),
        ("full negative token", TOKEN_NEGATIVE),
        ("full unknown token", TOKEN_UNKNOWN),
        ("full other port", TOKEN_OTHER_PORT),
        ("full other ip", TOKEN_OTHER_IP),
    ] {
        wire.ask(name, &status(session, token, &[0; 4]), bound, out)
            .await;
    }
    wire.ask(
        "full padding bytes set",
        &status(session, TOKEN_OK, &[0xff, 1, 2, 3]),
        bound,
        out,
    )
    .await;
    for length in [1usize, 2, 3, 5, 8] {
        let name = format!("padding of {length}");
        wire.ask(
            &name,
            &status(session, TOKEN_OK, &vec![0; length]),
            bound,
            out,
        )
        .await;
    }
    for (name, session) in [
        ("basic session 0", 0),
        ("basic session -1", -1),
        ("basic session max", i32::MAX),
        ("basic session min", i32::MIN),
    ] {
        wire.ask(name, &status(session, TOKEN_OK, &[]), bound, out)
            .await;
    }
    wire.ask(
        "full session min",
        &status(i32::MIN, TOKEN_OK, &[0; 4]),
        bound,
        out,
    )
    .await;
}

/// A basic and a full request with a good token, under the heading `state`.
async fn state_scenes(wire: &Wire, state: &str, bound: SocketAddr, out: &mut String) {
    let basic = format!("{state}: basic");
    wire.ask(&basic, &status(7, TOKEN_OK, &[]), bound, out)
        .await;
    let full = format!("{state}: full");
    wire.ask(&full, &status(7, TOKEN_OK, &[0; 4]), bound, out)
        .await;
}

/// A player named `name` (fixed identity) in world number `world`.
fn join(
    server: &Server,
    world: usize,
    name: &'static str,
    number: u128,
    taps: &mut Vec<crate::net::java::PacketTap>,
) {
    let world = server.worlds.load()[world].clone();
    let who = Identity {
        name,
        uuid: Uuid::from_u128(0x0000_0f00 + number),
        entity_id: 9_700 + number as i32,
    };
    if let Some((player, tap)) = test_support::java_player_as(&world, GameMode::Survival, &who) {
        let _ = world.add_player(&player);
        taps.push(tap);
    }
}

async fn plugin(server: &Arc<Server>, file: &str) {
    let manager = server.plugin_manager.clone();
    let _ = manager.try_load_plugin(server, Path::new(file)).await;
}

/// The server that fills up: players in two worlds, then plugins.
async fn filling_server(out: &mut String) -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let wire = Wire::new(server.clone()).await?;
    let ipv4 = addr("127.0.0.1:25565");
    let _ = writeln!(out, "worlds {}", server.worlds.load().len());
    malformed_scenes(&wire, ipv4, out).await;
    handshake_scenes(&wire, ipv4, out).await;
    request_scenes(&wire, ipv4, out).await;
    state_scenes(&wire, "empty", ipv4, out).await;
    let mut taps = Vec::new();
    join(&server, 0, "Alpha", 1, &mut taps);
    state_scenes(&wire, "one player", addr("10.1.2.3:65535"), out).await;
    join(&server, 0, "Bad\0Name", 2, &mut taps);
    join(&server, 0, "Cara", 3, &mut taps);
    state_scenes(&wire, "nul name", addr("[::1]:19132"), out).await;
    join(&server, 0, "Dane", 4, &mut taps);
    join(&server, 0, "Eve", 5, &mut taps);
    state_scenes(&wire, "five in world 0", ipv4, out).await;
    join(&server, 1, "Gus", 6, &mut taps);
    join(&server, 1, "Hal", 7, &mut taps);
    state_scenes(&wire, "two in world 1", ipv4, out).await;
    server
        .plugin_manager
        .add_loader(&server, Arc::new(StubLoader))
        .await;
    plugin(&server, "Alpha.stub").await;
    state_scenes(&wire, "one plugin", ipv4, out).await;
    plugin(&server, "Beta.stub").await;
    plugin(&server, "Gamma.stub").await;
    state_scenes(&wire, "three plugins", ipv4, out).await;
    plugin(&server, "Bad\0Plugin.stub").await;
    state_scenes(&wire, "nul plugin", ipv4, out).await;
    server.stop();
    Ok(())
}

/// A server with a NUL in the MOTD: every status reply fails to encode.
async fn bad_motd_server(out: &mut String) -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server_with_configs(
        &folder,
        |_| {},
        |advanced| advanced.networking.java.motd = "Bad\0Motd".to_owned(),
    )
    .await?;
    let wire = Wire::new(server.clone()).await?;
    state_scenes(&wire, "nul motd", addr("127.0.0.1:25565"), out).await;
    server.stop();
    Ok(())
}

/// A server with another MOTD and player limit, and a port that needs both bytes.
async fn custom_server(out: &mut String) -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server_with_configs(
        &folder,
        |_| {},
        |advanced| {
            advanced.networking.java.motd = "Kvasir test".to_owned();
            advanced.networking.java.max_players = 300;
        },
    )
    .await?;
    let wire = Wire::new(server.clone()).await?;
    state_scenes(&wire, "custom", addr("192.168.0.9:25566"), out).await;
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn query_replies_are_the_same() -> TestResult {
    let mut out = String::new();
    filling_server(&mut out).await?;
    bad_motd_server(&mut out).await?;
    custom_server(&mut out).await?;
    script_golden::bless_or_compare("query.txt", &out)
}
