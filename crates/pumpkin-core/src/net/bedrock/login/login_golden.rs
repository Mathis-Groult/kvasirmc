//! A record of what `BedrockClient::try_handle_login` does, made before the function was split
//! (`tests/golden/bedrock_login.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene starts a real server with the scene's configuration (online mode, enforced
//! authentication, name prefix, resource packs, player limit, whitelist, OIDC keys), puts a fresh
//! Bedrock client over an offline `NetherNet` session (with or without a proven identity key) and
//! an optional player already online, and hands the client one login packet built from fixed
//! test keys (no network; the signed tokens expire in the year 5138, so no clock matters). One
//! entry per scene: the returned result (the profile and config, `Stop`, or the error), the stored
//! client data, the user cache entry, the packets queued for the client (id, length, FNV-1a) and
//! whether the client was closed.
//! Not covered: the `NetherNet` login of a real peer (the session is never connected).

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bytes::Bytes;
use pumpkin_auth::jwt::{Jwk, Jwks, OIDC_AUDIENCE};
use pumpkin_auth::p384::PublicKey;
use pumpkin_auth::p384::ecdsa::signature::Signer;
use pumpkin_auth::p384::ecdsa::{Signature, SigningKey};
use pumpkin_auth::p384::pkcs8::EncodePublicKey;
use pumpkin_config::resource_pack::BedrockPack;
use pumpkin_protocol::bedrock::server::login::SLogin;
use pumpkin_util::GameMode;
use serde_json::json;
use uuid::Uuid;

use crate::entity::ai::test_support::{self, Identity};
use crate::net::bedrock::nethernet::offline_session_with_key;
use crate::net::bedrock::{BedrockClient, BedrockPacketTap};
use crate::net::packet_limiter::PacketRateLimiter;
use crate::net::{GameProfile, PacketHandlerResult};
use crate::server::Server;

use super::LoginError;

const ISSUER: &str = "https://issuer.test";

#[derive(Clone, Copy)]
enum Jwt {
    /// A chain wrapper with this authentication type (0 full, 1 guest, 2 self-signed).
    Typed(u8),
    NotJson,
    MissingToken,
}

#[derive(Clone, Copy)]
enum Tok {
    /// An unsigned token for the gamertag `Steve Smith` (offline mode reads it unchecked).
    Plain,
    Named(&'static str),
    LegUuid(&'static str),
    Anonymous,
    NoCpk,
    BadCpk,
    /// Signed with test key A (header carries `kid` and `x5u`).
    Signed,
    /// Header and `x5u` of key A, signature made with key B.
    BadSignature,
    Expired,
    NoDots,
    TwoParts,
    BadPayload,
}

#[derive(Clone, Copy)]
enum Raw {
    Good(&'static str),
    NotUtf8,
    NoDot,
    BadBase64,
    NotClientData,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Net {
    None,
    Same,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Occupant {
    None,
    SameUuid,
    SameName,
    Filler,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    jwt: Jwt,
    tok: Tok,
    raw: Raw,
    online: bool,
    auth_enforced: bool,
    prefix: &'static str,
    replace_spaces: bool,
    packs: Option<bool>,
    max_players: u32,
    whitelist: bool,
    oidc_keys: bool,
    net: Net,
    occupant: Occupant,
}

const BASE: Scene = Scene {
    name: "",
    jwt: Jwt::Typed(2),
    tok: Tok::Plain,
    raw: Raw::Good("en_US"),
    online: false,
    auth_enforced: false,
    prefix: "",
    replace_spaces: true,
    packs: None,
    max_players: 1000,
    whitelist: false,
    oidc_keys: false,
    net: Net::None,
    occupant: Occupant::None,
};

fn key(seed: u8) -> Result<SigningKey, String> {
    SigningKey::from_slice(&[seed; 48]).map_err(|error| error.to_string())
}

fn public_der_b64(key: &SigningKey) -> Result<String, String> {
    let der = PublicKey::from(key.verifying_key())
        .to_public_key_der()
        .map_err(|error| error.to_string())?;
    Ok(STANDARD.encode(der.as_bytes()))
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sign(signer: &SigningKey, input: &str) -> String {
    let signature: Signature = signer.sign(input.as_bytes());
    b64(&signature.to_bytes())
}

fn payload(tok: Tok, cpk: &str) -> serde_json::Value {
    let (name, xid, leguuid) = match tok {
        Tok::Named(name) => (name, "2535400000000001", ""),
        Tok::LegUuid(uuid) => ("Steve Smith", "", uuid),
        Tok::Anonymous => ("Steve Smith", "", ""),
        _ => ("Steve Smith", "2535400000000001", ""),
    };
    let mut value = json!({
        "xname": name,
        "xid": xid,
        "iss": ISSUER,
        "aud": OIDC_AUDIENCE,
        "exp": if matches!(tok, Tok::Expired) { 1u64 } else { 99_999_999_999u64 },
        "cpk": cpk,
    });
    if !leguuid.is_empty() {
        value["leguuid"] = json!(leguuid);
    }
    if matches!(tok, Tok::NoCpk) {
        value.as_object_mut().map(|object| object.remove("cpk"));
    }
    if matches!(tok, Tok::BadCpk) {
        value["cpk"] = json!("!!!");
    }
    value
}

fn token(tok: Tok) -> Result<String, String> {
    let (key_a, key_b) = (key(7)?, key(9)?);
    let cpk = public_der_b64(&key_a)?;
    match tok {
        Tok::NoDots => return Ok("nodots".to_string()),
        Tok::TwoParts => return Ok("aGVhZGVy.cGF5bG9hZA".to_string()),
        Tok::BadPayload => return Ok(format!("{}.!!!.c2ln", b64(b"{}"))),
        _ => {}
    }
    let header = json!({"alg": "ES384", "kid": "k1", "x5u": cpk});
    let body = format!(
        "{}.{}",
        b64(header.to_string().as_bytes()),
        b64(payload(tok, &cpk).to_string().as_bytes())
    );
    let signature = match tok {
        Tok::Signed | Tok::Expired => sign(&key_a, &body),
        Tok::BadSignature => sign(&key_b, &body),
        _ => "c2ln".to_string(),
    };
    Ok(format!("{body}.{signature}"))
}

fn login_packet(scene: &Scene) -> Result<SLogin, String> {
    let jwt = match scene.jwt {
        Jwt::NotJson => b"not json".to_vec(),
        Jwt::MissingToken => json!({"AuthenticationType": 0}).to_string().into_bytes(),
        Jwt::Typed(kind) => json!({"AuthenticationType": kind, "Token": token(scene.tok)?})
            .to_string()
            .into_bytes(),
    };
    Ok(SLogin {
        protocol_version: 944,
        jwt,
        raw_token: raw_token(scene.raw),
    })
}

fn raw_token(raw: Raw) -> Vec<u8> {
    let client = |lang: &str| {
        json!({
            "ClientRandomId": 1i64, "DeviceOS": 7, "DeviceId": "device", "GameVersion": "1.26.10",
            "LanguageCode": lang, "CurrentInputMode": 2, "DefaultInputMode": 2, "UIProfile": 0,
            "ServerAddress": "play.test:19132", "MaxViewDistance": 16
        })
    };
    match raw {
        Raw::Good(lang) => format!(
            "{}.{}.c2ln",
            b64(b"{}"),
            b64(client(lang).to_string().as_bytes())
        )
        .into_bytes(),
        Raw::NotUtf8 => vec![0xff, 0xfe, 0x2e],
        Raw::NoDot => b"abc".to_vec(),
        Raw::BadBase64 => b"aGVhZGVy.!!!.c2ln".to_vec(),
        Raw::NotClientData => format!("{}.{}.c2ln", b64(b"{}"), b64(b"{}")).into_bytes(),
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn var_uint(bytes: &[u8]) -> Option<(u32, usize)> {
    let (mut value, mut shift) = (0u32, 0);
    for (index, byte) in bytes.iter().enumerate().take(5) {
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
    }
    None
}

/// `id:length:hash` of a queued Bedrock packet (`VarUInt` length, `VarUInt` header, payload).
fn packet_text(packet: &[u8]) -> String {
    let id = var_uint(packet)
        .and_then(|(_, used)| var_uint(&packet[used..]))
        .map_or_else(
            || "?".to_string(),
            |(header, _)| (header & 0x3ff).to_string(),
        );
    format!("{id}:{}:{:08x}", packet.len(), fnv(packet) as u32)
}

/// A Bedrock tap drained by a task while the login runs: a kick waits until the writer took its
/// packet, and in a test nothing else reads the queue.
struct Drained {
    packets: Arc<Mutex<Vec<Bytes>>>,
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drained {
    fn start(mut tap: BedrockPacketTap) -> Self {
        let packets = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn({
            let packets = Arc::clone(&packets);
            let stop = Arc::clone(&stop);
            async move {
                loop {
                    let done = stop.load(Ordering::Acquire);
                    let drained = tap.drain();
                    packets
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(drained);
                    if done {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
        });
        Self {
            packets,
            stop,
            task,
        }
    }

    /// Everything received, after one last drain pass.
    async fn finish(self) -> Vec<String> {
        self.stop.store(true, Ordering::Release);
        let _ = self.task.await;
        self.packets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|packet| packet_text(packet))
            .collect()
    }
}

fn pack(uuid: u128, version: &str, scripts: bool) -> BedrockPack {
    BedrockPack {
        uuid: Uuid::from_u128(uuid),
        version: version.to_string(),
        size: 1234,
        download_url: "https://packs.test/a.mcpack".to_string(),
        path: None,
        content_key: "key".to_string(),
        sub_pack_name: "sub".to_string(),
        content_id: "content".to_string(),
        has_scripts: scripts,
        addon_pack: scripts,
        rtx_enabled: !scripts,
    }
}

async fn scene_server(scene: &Scene, folder: &tempfile::TempDir) -> Result<Arc<Server>, String> {
    let scene = *scene;
    let server = test_support::server_with_configs(
        folder,
        |_| {},
        move |advanced| {
            let bedrock = &mut advanced.networking.bedrock;
            bedrock.online_mode = scene.online;
            bedrock.authentication.enabled = scene.auth_enforced;
            bedrock.username_prefix = scene.prefix.to_string();
            bedrock.replace_username_spaces = scene.replace_spaces;
            bedrock.max_players = scene.max_players;
            if let Some(force) = scene.packs {
                let packs = &mut advanced.resource_pack.bedrock;
                packs.enabled = true;
                packs.force = force;
                packs.packs = vec![pack(0xa1, "1.0.0", false), pack(0xa2, "2.1.0", true)];
            }
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    server.white_list.store(scene.whitelist, Ordering::Relaxed);
    if scene.oidc_keys {
        let x_y = public_der_b64(&key(7)?)?;
        let der = STANDARD.decode(x_y).map_err(|error| error.to_string())?;
        let point = &der[der.len() - 96..];
        let jwk = Jwk {
            kty: "EC".to_string(),
            alg: Some("ES384".to_string()),
            crv: Some("P-384".to_string()),
            x: Some(b64(&point[..48])),
            y: Some(b64(&point[48..])),
            n: None,
            e: None,
            kid: "k1".to_string(),
        };
        let keys = Jwks { keys: vec![jwk] };
        server
            .bedrock_oidc_keys
            .set((ISSUER.to_string(), keys))
            .map_err(|_| "OIDC keys already set".to_string())?;
    }
    Ok(server)
}

/// The player that is online when the login arrives.
async fn add_occupant(server: &Server, occupant: Occupant) -> Result<(), String> {
    let (name, uuid) = match occupant {
        Occupant::None => return Ok(()),
        Occupant::SameUuid => ("Somebody", steve_uuid()?),
        Occupant::SameName => ("Steve_Smith", Uuid::from_u128(0xdd)),
        Occupant::Filler => ("Somebody", Uuid::from_u128(0xdd)),
    };
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let who = Identity {
        name,
        uuid,
        entity_id: 9_501,
    };
    let (player, _tap) = test_support::bedrock_player_as(&world, GameMode::Survival, &who).await?;
    world.add_player(&player)
}

fn steve_uuid() -> Result<Uuid, String> {
    let claims = pumpkin_auth::jwt::extract_oidc_token_player_claims(&token(Tok::Plain)?)
        .map_err(|error| error.to_string())?;
    Uuid::parse_str(&claims.uuid).map_err(|error| error.to_string())
}

async fn login_client(net: Net) -> Result<(Arc<BedrockClient>, BedrockPacketTap), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], 19132));
    let identity = match net {
        Net::None => None,
        Net::Same => Some(PublicKey::from(key(7)?.verifying_key())),
        Net::Other => Some(PublicKey::from(key(9)?.verifying_key())),
    };
    let session = offline_session_with_key(address, identity).await?;
    let client = Arc::new(BedrockClient::new(
        session,
        address,
        Arc::default(),
        PacketRateLimiter::new(false, 0.0, 0.0),
    ));
    let tap = client
        .take_packet_tap()
        .ok_or("a new Bedrock client no longer owned its outgoing queue")?;
    Ok((client, tap))
}

fn result_text(result: &Result<PacketHandlerResult, LoginError>) -> String {
    match result {
        Ok(PacketHandlerResult::Stop) => "Stop".to_string(),
        Ok(PacketHandlerResult::ReadyToPlay(profile, config)) => profile_text(profile, config),
        Err(error) => format!("Err {error:?}"),
    }
}

fn profile_text(profile: &GameProfile, config: &crate::net::PlayerConfig) -> String {
    format!(
        "ReadyToPlay id {} name {} props {} actions {} locale {} view {}",
        profile.id,
        profile.name,
        profile.properties.load().len(),
        profile.profile_actions.is_some(),
        config.locale,
        config.view_distance,
    )
}

async fn run_scene(scene: Scene) -> Result<String, String> {
    let folder = tempfile::tempdir().map_err(|error| error.to_string())?;
    let server = scene_server(&scene, &folder).await?;
    add_occupant(&server, scene.occupant).await?;
    let (client, tap) = login_client(scene.net).await?;
    let drained = Drained::start(tap);
    let result = client
        .try_handle_login(login_packet(&scene)?, &server)
        .await;
    let sent = drained.finish().await;
    let stored = client.client_data.load();
    let stored = stored.as_ref().as_ref().map_or_else(
        || "none".to_string(),
        |data| format!("{} {}", data.language_code, data.game_version),
    );
    let cached = server
        .data
        .user_cache
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_by_name(&format!("{}Steve_Smith", scene.prefix))
        .map_or_else(|| "none".to_string(), |entry| entry.uuid.to_string());
    let mut text = String::new();
    let _ = writeln!(text, "{}", scene.name);
    let _ = writeln!(text, "  result: {}", result_text(&result));
    let _ = writeln!(text, "  client data: {stored}");
    let _ = writeln!(text, "  user cache: {cached}");
    let _ = writeln!(text, "  sent: {sent:?}");
    let _ = writeln!(text, "  closed: {}", client.is_closed());
    server.stop();
    Ok(text)
}

fn chain_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "chain: not JSON",
            jwt: Jwt::NotJson,
            ..BASE
        },
        Scene {
            name: "chain: no token field",
            jwt: Jwt::MissingToken,
            ..BASE
        },
        Scene {
            name: "chain: unknown authentication type",
            jwt: Jwt::Typed(9),
            ..BASE
        },
        Scene {
            name: "offline: plain token",
            ..BASE
        },
        Scene {
            name: "offline: two-part token",
            tok: Tok::TwoParts,
            ..BASE
        },
        Scene {
            name: "offline: token without dots",
            tok: Tok::NoDots,
            ..BASE
        },
        Scene {
            name: "offline: payload not base64",
            tok: Tok::BadPayload,
            ..BASE
        },
        Scene {
            name: "offline: guest type is not checked",
            jwt: Jwt::Typed(1),
            ..BASE
        },
        Scene {
            name: "online full: no OIDC keys",
            jwt: Jwt::Typed(0),
            online: true,
            ..BASE
        },
        Scene {
            name: "online full: signed token, keys loaded",
            jwt: Jwt::Typed(0),
            tok: Tok::Signed,
            online: true,
            oidc_keys: true,
            ..BASE
        },
        Scene {
            name: "online full: bad signature",
            jwt: Jwt::Typed(0),
            tok: Tok::BadSignature,
            online: true,
            oidc_keys: true,
            ..BASE
        },
        Scene {
            name: "online guest",
            jwt: Jwt::Typed(1),
            online: true,
            ..BASE
        },
        Scene {
            name: "online self-signed while authentication is enforced",
            tok: Tok::Signed,
            online: true,
            auth_enforced: true,
            ..BASE
        },
        Scene {
            name: "online self-signed: valid",
            tok: Tok::Signed,
            online: true,
            ..BASE
        },
        Scene {
            name: "online self-signed: bad signature",
            tok: Tok::BadSignature,
            online: true,
            ..BASE
        },
        Scene {
            name: "online self-signed: expired",
            tok: Tok::Expired,
            online: true,
            ..BASE
        },
        Scene {
            name: "online self-signed: plain token",
            online: true,
            ..BASE
        },
    ]
}

fn identity_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "raw token: not UTF-8",
            raw: Raw::NotUtf8,
            ..BASE
        },
        Scene {
            name: "raw token: no dot",
            raw: Raw::NoDot,
            ..BASE
        },
        Scene {
            name: "raw token: payload not base64",
            raw: Raw::BadBase64,
            ..BASE
        },
        Scene {
            name: "raw token: payload lacks client fields",
            raw: Raw::NotClientData,
            ..BASE
        },
        Scene {
            name: "raw token: other language",
            raw: Raw::Good("fr_FR"),
            ..BASE
        },
        Scene {
            name: "uuid: legacy uuid",
            tok: Tok::LegUuid("00000000-0000-0000-0000-0000000000ab"),
            ..BASE
        },
        Scene {
            name: "uuid: invalid legacy uuid",
            tok: Tok::LegUuid("not-a-uuid"),
            ..BASE
        },
        Scene {
            name: "uuid: anonymous",
            tok: Tok::Anonymous,
            ..BASE
        },
        Scene {
            name: "key: token without cpk",
            tok: Tok::NoCpk,
            ..BASE
        },
        Scene {
            name: "key: token with bad cpk",
            tok: Tok::BadCpk,
            ..BASE
        },
        Scene {
            name: "name: prefix, spaces replaced",
            prefix: "BE_",
            ..BASE
        },
        Scene {
            name: "name: prefix, spaces kept",
            prefix: "BE_",
            replace_spaces: false,
            ..BASE
        },
        Scene {
            name: "name: no spaces",
            tok: Tok::Named("Alex"),
            ..BASE
        },
    ]
}

fn gate_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "nethernet: same key",
            net: Net::Same,
            ..BASE
        },
        Scene {
            name: "nethernet: other key",
            net: Net::Other,
            ..BASE
        },
        Scene {
            name: "gate: server full",
            max_players: 1,
            occupant: Occupant::Filler,
            ..BASE
        },
        Scene {
            name: "gate: one slot left",
            max_players: 2,
            occupant: Occupant::Filler,
            ..BASE
        },
        Scene {
            name: "gate: not whitelisted",
            whitelist: true,
            ..BASE
        },
        Scene {
            name: "gate: duplicate uuid",
            occupant: Occupant::SameUuid,
            ..BASE
        },
        Scene {
            name: "gate: duplicate name",
            occupant: Occupant::SameName,
            ..BASE
        },
        Scene {
            name: "packs: optional",
            packs: Some(false),
            ..BASE
        },
        Scene {
            name: "packs: forced",
            packs: Some(true),
            ..BASE
        },
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_login() -> Result<(), Box<dyn std::error::Error>> {
    let mut scenes = chain_scenes();
    scenes.extend(identity_scenes());
    scenes.extend(gate_scenes());
    let tasks: Vec<_> = scenes
        .into_iter()
        .map(|scene| tokio::spawn(run_scene(scene)))
        .collect();
    let mut report = String::new();
    for task in tasks {
        report.push_str(&task.await??);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("bedrock_login.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
