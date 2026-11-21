//! A record of what `JavaClient::validate_chat_message` answers, made before the function was
//! split (`tests/golden/chat_validation.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One line per scene: the verdict (`ok` or the `ChatError` variant) for a message, with or
//! without secure chat (`allow_chat_reports`), a signature, a timestamp relative to now, the
//! session expiry, the message count, the acknowledged bits, the tracker's prepared entries and
//! the checksum. The player's tracker is shown afterwards (entries kept / pending).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SChatMessage;
use pumpkin_util::GameMode;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::player::LastSeenTrackedEntry;
use crate::net::ClientPlatform;
use crate::net::java::play::ChatError;

#[derive(Clone)]
struct Scene {
    name: &'static str,
    secure: bool,
    message: String,
    /// `None` = unsigned, else the signature length.
    signature: Option<usize>,
    /// Milliseconds added to now for the message timestamp.
    timestamp_offset: i64,
    /// Milliseconds added to now for the session expiry.
    expiry_offset: i64,
    count: i32,
    acknowledged: Vec<u8>,
    /// Entries pushed on the tracker before the message: `Some(pending)` or `None`.
    prepared: Vec<Option<bool>>,
    /// `0` = skip, `1` = the right value for the (empty) last seen list, `2` = a wrong one.
    checksum: u8,
}

fn scene(name: &'static str) -> Scene {
    Scene {
        name,
        secure: true,
        message: "hello".to_string(),
        signature: Some(256),
        timestamp_offset: -1_000,
        expiry_offset: 60_000,
        count: 0,
        acknowledged: Vec::new(),
        prepared: Vec::new(),
        checksum: 0,
    }
}

fn plain(name: &'static str, message: String) -> Scene {
    Scene {
        secure: false,
        message,
        signature: None,
        ..scene(name)
    }
}

fn text_scenes() -> Vec<Scene> {
    let long = |n: usize| "a".repeat(n);
    let mut scenes = vec![
        plain("text ok", "hello".into()),
        plain("text empty", String::new()),
        plain("text 256 chars", long(256)),
        plain("text 257 chars", long(257)),
        plain("text 128 emoji (256 utf16)", "😀".repeat(128)),
        plain("text 129 emoji (258 utf16)", "😀".repeat(129)),
        plain(
            "text 255 chars + emoji (257 utf16)",
            format!("{}😀", long(255)),
        ),
        plain("text section sign", "a§b".into()),
        plain("text newline", "a\nb".into()),
        plain("text unit separator", "a\u{1f}b".into()),
        plain("text space", " ".into()),
        plain("text tilde", "~".into()),
        plain("text delete", "a\u{7f}b".into()),
        plain("text accent", "é".into()),
        plain("text tab", "a\tb".into()),
    ];
    scenes.push(Scene {
        message: long(257),
        ..scene("secure: oversized wins over missing signature")
    });
    scenes.push(Scene {
        message: "a§".into(),
        signature: None,
        ..scene("secure: illegal wins over missing signature")
    });
    scenes
}

fn secure_scenes() -> Vec<Scene> {
    vec![
        scene("secure ok"),
        Scene {
            signature: None,
            ..scene("secure unsigned")
        },
        Scene {
            signature: Some(255),
            ..scene("secure signature 255")
        },
        Scene {
            signature: Some(257),
            ..scene("secure signature 257")
        },
        Scene {
            signature: Some(0),
            ..scene("secure signature empty")
        },
        Scene {
            timestamp_offset: 5_000,
            ..scene("secure timestamp in the future")
        },
        Scene {
            timestamp_offset: -180_000,
            ..scene("secure timestamp 3 minutes old")
        },
        Scene {
            timestamp_offset: -60_000,
            ..scene("secure timestamp 1 minute old")
        },
        Scene {
            expiry_offset: -1_000,
            ..scene("secure session expired")
        },
        Scene {
            signature: None,
            timestamp_offset: -180_000,
            ..scene("secure unsigned wins over old timestamp")
        },
        Scene {
            timestamp_offset: -180_000,
            expiry_offset: -1_000,
            ..scene("secure old timestamp wins over expiry")
        },
        Scene {
            expiry_offset: -1_000,
            count: -1,
            ..scene("secure expiry wins over negative count")
        },
        Scene {
            secure: false,
            ..scene("insecure ignores signature, timestamp and count")
        },
        Scene {
            secure: false,
            signature: None,
            timestamp_offset: -999_000,
            expiry_offset: -999_000,
            count: -5,
            checksum: 2,
            ..scene("insecure ignores everything")
        },
    ]
}

fn tracker_scenes() -> Vec<Scene> {
    let pending = |n: usize| vec![Some(true); n];
    vec![
        Scene {
            count: -1,
            ..scene("secure negative count")
        },
        Scene {
            count: 1,
            ..scene("secure count 1 with nothing to drain")
        },
        Scene {
            count: 3,
            prepared: pending(3),
            ..scene("secure count 3 with 3 prepared")
        },
        Scene {
            count: 4,
            prepared: pending(3),
            ..scene("secure count 4 with 3 prepared")
        },
        Scene {
            acknowledged: vec![0x01],
            ..scene("secure ack of an unknown message")
        },
        Scene {
            count: 5,
            prepared: pending(5),
            acknowledged: vec![0x00, 0x80, 0x0f],
            ..scene("secure ack of 5 prepared")
        },
        Scene {
            count: 5,
            prepared: pending(5),
            acknowledged: vec![0x00, 0x80, 0x07],
            ..scene("secure ack of 4 of 5 prepared")
        },
        Scene {
            count: 5,
            prepared: pending(5),
            acknowledged: vec![0x00, 0x00, 0x01],
            ..scene("secure ack of 1 of 5 prepared")
        },
        Scene {
            count: 5,
            prepared: pending(5),
            acknowledged: vec![0x00],
            ..scene("secure empty-looking ack bytes with 5 prepared")
        },
        Scene {
            count: 5,
            prepared: pending(5),
            ..scene("secure no ack with 5 prepared")
        },
        Scene {
            count: 5,
            prepared: vec![Some(false), Some(true), Some(true), Some(true), Some(true)],
            acknowledged: vec![0x00, 0x80, 0x0f],
            ..scene("secure ack with an already acknowledged entry in the window")
        },
        Scene {
            count: 5,
            prepared: vec![Some(false), Some(true), Some(true), Some(true), Some(true)],
            acknowledged: vec![0x00, 0x00, 0x0f],
            ..scene("secure ack ignoring an acknowledged entry")
        },
    ]
}

fn checksum_scenes() -> Vec<Scene> {
    let pending = |n: usize| vec![Some(true); n];
    vec![
        Scene {
            checksum: 1,
            ..scene("secure right checksum")
        },
        Scene {
            checksum: 2,
            ..scene("secure wrong checksum")
        },
        Scene {
            checksum: 2,
            count: -1,
            ..scene("secure negative count wins over wrong checksum")
        },
        Scene {
            checksum: 1,
            count: 3,
            prepared: pending(3),
            ..scene("secure right checksum after a drain")
        },
        Scene {
            prepared: vec![None; 4_076],
            ..scene("secure 4096 tracked")
        },
        Scene {
            prepared: vec![None; 4_077],
            ..scene("secure 4097 tracked")
        },
        Scene {
            prepared: vec![None; 4_200],
            count: 100,
            ..scene("secure 4220 tracked, count 100 drains 100")
        },
        Scene {
            prepared: vec![None; 4_300],
            count: 100,
            ..scene("secure 4320 tracked, count 100 drains 100 still over")
        },
    ]
}

const fn verdict(result: &Result<(), ChatError>) -> &'static str {
    match result {
        Ok(()) => "ok",
        Err(ChatError::OversizedMessage) => "OversizedMessage",
        Err(ChatError::IllegalCharacters) => "IllegalCharacters",
        Err(ChatError::UnsignedChat) => "UnsignedChat",
        Err(ChatError::TooManyPendingChats) => "TooManyPendingChats",
        Err(ChatError::ChatValidationFailed) => "ChatValidationFailed",
        Err(ChatError::OutOfOrderChat) => "OutOfOrderChat",
        Err(ChatError::ExpiredPublicKey) => "ExpiredPublicKey",
        Err(ChatError::InvalidPublicKey) => "InvalidPublicKey",
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn run(
    secure: &Arc<crate::server::Server>,
    insecure: &Arc<crate::server::Server>,
    scene: &Scene,
) -> Result<String, String> {
    let server = if scene.secure { secure } else { insecure };
    let world = server.worlds.load()[0].clone();
    let player = test_support::java_player(&world, GameMode::Survival);
    let now = now_ms();
    player
        .chat_session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .expires_at = now + scene.expiry_offset;
    {
        let mut cache = player
            .signature_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (index, entry) in scene.prepared.iter().enumerate() {
            cache
                .last_seen_validator
                .tracked_messages
                .push_back(entry.map(|pending| LastSeenTrackedEntry {
                    signature: vec![index as u8; 4].into_boxed_slice(),
                    pending,
                }));
        }
    }
    let signature = scene.signature.map(|len| vec![7u8; len]);
    let packet = SChatMessage {
        message: &scene.message,
        timestamp: now + scene.timestamp_offset,
        salt: 0,
        signature: signature.as_deref(),
        message_count: VarInt(scene.count),
        acknowledged: &scene.acknowledged,
        // The right value for an empty last-seen list is 1 (see `polynomial_rolling_hash`).
        checksum: scene.checksum,
    };
    let ClientPlatform::Java(client) = &*player.client else {
        return Err("not a Java client".into());
    };
    let result = client.validate_chat_message(server, &player, &packet);
    let cache = player
        .signature_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tracker = &cache.last_seen_validator.tracked_messages;
    let kept = tracker.iter().filter(|entry| entry.is_some()).count();
    let pending = tracker
        .iter()
        .filter(|entry| entry.as_ref().is_some_and(|e| e.pending))
        .count();
    Ok(format!(
        "{}: {} | tracked {} kept {kept} pending {pending}\n",
        scene.name,
        verdict(&result),
        tracker.len()
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_validation_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let secure =
        test_support::server_with(&folder, |basic| basic.allow_chat_reports = true).await?;
    let folder2 = tempfile::tempdir()?;
    let insecure =
        test_support::server_with(&folder2, |basic| basic.allow_chat_reports = false).await?;
    let mut scenes = text_scenes();
    scenes.extend(secure_scenes());
    scenes.extend(tracker_scenes());
    scenes.extend(checksum_scenes());
    let mut out = String::new();
    for scene in &scenes {
        out.push_str(&run(&secure, &insecure, scene)?);
    }
    secure.stop();
    insecure.stop();
    script_golden::bless_or_compare("chat_validation.txt", &out)
}
