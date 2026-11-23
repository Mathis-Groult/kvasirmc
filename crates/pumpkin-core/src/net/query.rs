use std::{
    collections::HashMap,
    ffi::{CString, NulError},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use pumpkin_protocol::query::{
    CBasicStatus, CFullStatus, CHandshake, PacketType, RawQueryPacket, SHandshake, SStatusRequest,
};
use pumpkin_util::text::{TextComponent, color::NamedColor};
use pumpkin_world::CURRENT_MC_VERSION;
use rand::RngExt;
use tokio::{net::UdpSocket, sync::RwLock, time};
use tracing::{error, info};

use crate::server::Server;

pub async fn start_query_handler(server: Arc<Server>, query_addr: SocketAddr) {
    let Ok(socket) = UdpSocket::bind(query_addr).await else {
        error!("Unable to bind query UDP socket");
        return;
    };
    let socket = Arc::new(socket);

    // Challenge tokens are bound to the IP address and port
    let valid_challenge_tokens = Arc::new(RwLock::new(HashMap::new()));
    let valid_challenge_tokens_clone = valid_challenge_tokens.clone();
    // All challenge tokens ever created are expired every 30 seconds
    let expiry_stop = server.stop_token.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                () = expiry_stop.cancelled() => break,
            }
            valid_challenge_tokens_clone.write().await.clear();
        }
    });

    if let Ok(local_addr) = socket.local_addr() {
        info!(
            "Server query running on port {}",
            TextComponent::text(format!("{}", local_addr.port()))
                .color_named(NamedColor::DarkBlue)
                .to_pretty_console()
        );
    }

    loop {
        let socket = socket.clone();
        let valid_challenge_tokens = valid_challenge_tokens.clone();
        let server = server.clone();
        let mut buf = vec![0; 1024];

        let recv_result = tokio::select! {
            result = socket.recv_from(&mut buf) => Some(result),
            () = server.stop_token.cancelled() => None,
        };

        let Some(Ok((length, addr))) = recv_result else {
            break;
        };

        buf.truncate(length);

        tokio::spawn(async move {
            if let Err(err) = handle_packet(
                buf,
                valid_challenge_tokens,
                server,
                socket,
                addr,
                query_addr,
            )
            .await
            {
                error!("Interior 0 bytes found! Cannot encode query response! {err}");
            }
        });
    }
}

// Errors of packets that don't meet the format aren't returned since we won't handle them anyway
// The only errors that are thrown are because of a null terminator in a CString
// since those errors need to be corrected by server owner
#[inline]
async fn handle_packet(
    buf: Vec<u8>,
    clients: Arc<RwLock<HashMap<i32, SocketAddr>>>,
    server: Arc<Server>,
    socket: Arc<UdpSocket>,
    addr: SocketAddr,
    bound_addr: SocketAddr,
) -> Result<(), NulError> {
    let Ok(mut raw_packet) = RawQueryPacket::decode(buf).await else {
        return Ok(());
    };
    match raw_packet.packet_type {
        PacketType::Handshake => {
            handle_handshake(&mut raw_packet, &clients, &socket, addr).await;
            Ok(())
        }
        PacketType::Status => {
            handle_status(
                &mut raw_packet,
                &clients,
                &server,
                &socket,
                (addr, bound_addr),
            )
            .await
        }
    }
}

/// Answers a handshake with a new challenge token and remembers it for the sender.
async fn handle_handshake(
    raw_packet: &mut RawQueryPacket,
    clients: &RwLock<HashMap<i32, SocketAddr>>,
    socket: &UdpSocket,
    addr: SocketAddr,
) {
    let Ok(packet) = SHandshake::decode(raw_packet).await else {
        return;
    };
    let challenge_token = rand::rng().random_range(1..=i32::MAX);
    let response = CHandshake {
        session_id: packet.session_id,
        challenge_token,
    };

    // Ignore all errors since we don't want the query handler to crash
    // Protocol also ignores all errors and just doesn't respond
    if let Some(encoded) = response.encode() {
        let _ = socket.send_to(encoded.as_slice(), addr).await;
    }

    clients.write().await.insert(challenge_token, addr);
}

/// Answers a basic or full status request whose challenge token was given to the sender.
async fn handle_status(
    raw_packet: &mut RawQueryPacket,
    clients: &RwLock<HashMap<i32, SocketAddr>>,
    server: &Server,
    socket: &UdpSocket,
    (addr, bound_addr): (SocketAddr, SocketAddr),
) -> Result<(), NulError> {
    let Ok(packet) = SStatusRequest::decode(raw_packet).await else {
        return Ok(());
    };
    if !token_is_bound(clients, packet.challenge_token, addr).await {
        return Ok(());
    }
    let encoded = if packet.is_full_request {
        full_status(server, packet.session_id, bound_addr)?.encode()
    } else {
        basic_status(server, packet.session_id, bound_addr)?.encode()
    };
    if let Some(encoded) = encoded {
        let _ = socket.send_to(encoded.as_slice(), addr).await;
    }
    Ok(())
}

/// Whether `token` was handed to exactly this address.
async fn token_is_bound(
    clients: &RwLock<HashMap<i32, SocketAddr>>,
    token: i32,
    addr: SocketAddr,
) -> bool {
    clients
        .read()
        .await
        .get(&token)
        .is_some_and(|token_bound_ip: &SocketAddr| token_bound_ip == &addr)
}

/// The name of the first world, as the status replies show it.
fn map_name(server: &Server) -> Result<CString, NulError> {
    CString::new(
        server
            .worlds
            .load()
            .first()
            .map_or("world", |w| w.get_world_name()),
    )
}

/// The names of up to 4 players, taken world by world.
fn player_names(server: &Server) -> Vec<CString> {
    let mut players: Vec<CString> = Vec::new();
    for world in server.worlds.load().iter() {
        players.extend(
            world
                .players
                .load()
                // Although there is no documented limit, we will limit to 4 players
                .iter()
                .take(4 - players.len())
                .filter_map(|player| CString::new(player.gameprofile.name.as_str()).ok()),
        );
    }
    players
}

fn full_status(
    server: &Server,
    session_id: i32,
    bound_addr: SocketAddr,
) -> Result<CFullStatus, NulError> {
    let plugins = server
        .plugin_manager
        .active_plugins()
        .into_iter()
        .map(|meta| meta.name)
        .reduce(|acc, name| format!("{acc}, {name}"))
        .unwrap_or_default();

    Ok(CFullStatus {
        session_id,
        hostname: CString::new(server.advanced_config.networking.java.motd.as_str())?,
        version: CString::new(CURRENT_MC_VERSION)?,
        plugins: CString::new(plugins)?,
        map: map_name(server)?,
        num_players: server.get_player_count(),
        max_players: server.advanced_config.networking.java.max_players as usize,
        host_port: bound_addr.port(),
        host_ip: CString::new(bound_addr.ip().to_string())?,
        players: player_names(server),
    })
}

fn basic_status(
    server: &Server,
    session_id: i32,
    bound_addr: SocketAddr,
) -> Result<CBasicStatus, NulError> {
    Ok(CBasicStatus {
        session_id,
        motd: CString::new(server.advanced_config.networking.java.motd.as_str())?,
        map: map_name(server)?,
        num_players: server.get_player_count(),
        max_players: server.advanced_config.networking.java.max_players as usize,
        host_port: bound_addr.port(),
        host_ip: CString::new(bound_addr.ip().to_string())?,
    })
}

#[cfg(test)]
mod golden;
