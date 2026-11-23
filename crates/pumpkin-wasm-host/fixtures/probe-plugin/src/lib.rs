//! Probe plugin: `on_load` does what `PROBE_MODE` (set by the host) says. Mode `ok`, `err` and
//! `trap` end the call in the three ways a guest can; any other mode returns a report of what the
//! sandbox allowed, as `Err(report)`, one `key=value` line per probe.

use std::{
    fmt::Write as _,
    io::ErrorKind,
    net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
};

use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, register_plugin};

struct Probe;

fn verdict<T>(result: std::io::Result<T>) -> &'static str {
    match result {
        Ok(_) => "allowed",
        Err(error) if error.kind() == ErrorKind::PermissionDenied => "denied",
        Err(_) => "failed",
    }
}

fn env_line(out: &mut String, key: &str) {
    match std::env::var(key) {
        Ok(value) if key == "PATH" => {
            let _ = writeln!(
                out,
                "env.{key}={}",
                if value.is_empty() { "empty" } else { "set" }
            );
        }
        Ok(value) => {
            let _ = writeln!(out, "env.{key}={value}");
        }
        Err(_) => {
            let _ = writeln!(out, "env.{key}=-");
        }
    }
}

fn fs_lines(out: &mut String) {
    let _ = writeln!(out, "fs.read={}", verdict(std::fs::read_dir("data")));
    let _ = writeln!(
        out,
        "fs.write={}",
        verdict(std::fs::write("data/probe.txt", b"probe"))
    );
    let _ = std::fs::remove_file("data/probe.txt");
}

fn net_lines(out: &mut String) {
    let port = std::env::var("PROBE_PORT").unwrap_or_default();
    let _ = writeln!(
        out,
        "tcp.connect_loopback={}",
        verdict(TcpStream::connect(format!("127.0.0.1:{port}")))
    );
    let _ = writeln!(
        out,
        "tcp.bind_loopback={}",
        verdict(TcpListener::bind("127.0.0.1:0"))
    );
    let _ = writeln!(
        out,
        "tcp.bind_any={}",
        verdict(TcpListener::bind("0.0.0.0:0"))
    );
    let _ = writeln!(
        out,
        "udp.bind_loopback={}",
        verdict(UdpSocket::bind("127.0.0.1:0"))
    );
    let _ = writeln!(
        out,
        "udp.bind_any={}",
        verdict(UdpSocket::bind("0.0.0.0:0"))
    );
    let send_public = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.send_to(b"p", "192.0.2.1:9"));
    let _ = writeln!(out, "udp.send_public={}", verdict(send_public));
    let send_loopback = UdpSocket::bind("127.0.0.1:0").and_then(|s| s.send_to(b"p", "127.0.0.1:9"));
    let _ = writeln!(out, "udp.send_loopback={}", verdict(send_loopback));
    let _ = writeln!(out, "dns={}", verdict(("localhost", 80).to_socket_addrs()));
}

fn memory_line(out: &mut String) {
    let megabytes = 64_usize;
    let mut block: Vec<u8> = Vec::new();
    let grown = block.try_reserve_exact(megabytes * 1024 * 1024).is_ok();
    let _ = writeln!(
        out,
        "memory.{megabytes}mb={}",
        if grown { "allowed" } else { "denied" }
    );
}

impl Plugin for Probe {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "probe".into(),
            version: "0.1.0".into(),
            authors: vec![],
            description: "Reports what its sandbox allows.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, _context: Context) -> Result<(), String> {
        let mode = std::env::var("PROBE_MODE").unwrap_or_default();
        match mode.as_str() {
            "ok" => Ok(()),
            "err" => Err("guest refused to load".into()),
            "trap" => std::process::abort(),
            _ => {
                let mut out = String::new();
                for key in ["PATH", "PROBE_MODE", "PROBE_INJECTED"] {
                    env_line(&mut out, key);
                }
                fs_lines(&mut out);
                net_lines(&mut out);
                memory_line(&mut out);
                Err(out)
            }
        }
    }
}

register_plugin!(Probe);
