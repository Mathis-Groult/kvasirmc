//! Golden for `runtime::on_load`: the probe plugin (`fixtures/probe-plugin`, rebuilt by its
//! `build.sh`) is loaded through `PluginRuntime::init_plugin` and `on_load` runs it under many
//! permission lists, server configurations and per-plugin overrides. Each scenario prints how the
//! guest call ended, what the guest saw of its sandbox (environment, data folder, sockets, DNS,
//! memory) and the state `on_load` left in the store. One test per group of scenarios, so each
//! stays well under the test-time budget. Bless with `BLESS_ON_LOAD=1`.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    net::TcpListener,
    path::Path,
    sync::{Arc, OnceLock},
};

use pumpkin_config::{
    AdvancedConfiguration, BasicConfiguration, TelemetryConfig,
    plugins::{PluginOverride, PluginsConfig},
};
use pumpkin_core::{
    data::VanillaData,
    plugin::{Context, PluginManager, PluginMetadata, permissions},
    server::Server,
};
use pumpkin_wasm_host_common::{
    concurrent_store::{LegacySyncReentry, TokioSpawner},
    scheduler::TaskScheduler,
};

use crate::runtime::{PluginRuntime, on_load};

const PROBE: &[u8] = include_bytes!("../fixtures/probe.wasm");

type Tweak = fn(&mut PluginsConfig, &mut Option<PluginOverride>);

struct Scenario {
    name: &'static str,
    permissions: Vec<String>,
    tweak: Tweak,
}

fn scenario(name: &'static str, permissions: &[&str], tweak: Tweak) -> Scenario {
    Scenario {
        name,
        permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        tweak,
    }
}

fn untouched(_: &mut PluginsConfig, _: &mut Option<PluginOverride>) {}

fn over(slot: &mut Option<PluginOverride>) -> &mut PluginOverride {
    slot.get_or_insert_default()
}

fn mode_scenarios() -> Vec<Scenario> {
    vec![
        scenario("bare", &[], untouched),
        scenario("mode_ok", &[], |_, o| {
            over(o).environment.insert("PROBE_MODE".into(), "ok".into());
        }),
        scenario("mode_err", &[], |_, o| {
            over(o)
                .environment
                .insert("PROBE_MODE".into(), "err".into());
        }),
        scenario("mode_trap", &[], |_, o| {
            over(o)
                .environment
                .insert("PROBE_MODE".into(), "trap".into());
        }),
        scenario("injected_env", &[], |_, o| {
            over(o)
                .environment
                .insert("PROBE_INJECTED".into(), "yes".into());
        }),
        scenario("override_of_other_plugin", &[], |c, _| {
            c.overrides.insert(
                "somebody_else".into(),
                PluginOverride {
                    max_memory_mb: Some(1),
                    environment: HashMap::from([("PROBE_INJECTED".into(), "leak".into())]),
                    ..PluginOverride::default()
                },
            );
        }),
    ]
}

fn tcp_scenarios() -> Vec<Scenario> {
    vec![
        scenario("dns", &[permissions::NETWORK_DNS], untouched),
        scenario("tcp", &[permissions::NETWORK_TCP], |_, o| {
            over(o);
        }),
        scenario("tcp_no_override", &[permissions::NETWORK_TCP], untouched),
        scenario(
            "tcp_connect",
            &[permissions::NETWORK_TCP_CONNECT],
            |_, o| {
                over(o);
            },
        ),
        scenario("tcp_bind", &[permissions::NETWORK_TCP_BIND], |_, o| {
            over(o);
        }),
    ]
}

fn udp_scenarios() -> Vec<Scenario> {
    vec![
        scenario("udp", &[permissions::NETWORK_UDP], untouched),
        scenario(
            "udp_connect",
            &[permissions::NETWORK_UDP_CONNECT],
            untouched,
        ),
        scenario("udp_bind", &[permissions::NETWORK_UDP_BIND], untouched),
        scenario(
            "udp_outgoing",
            &[permissions::NETWORK_UDP_OUTGOING_DATAGRAM],
            untouched,
        ),
        scenario("outbound", &[permissions::NETWORK_OUTBOUND], |_, o| {
            over(o);
        }),
        scenario(
            "loopback_perm_only",
            &[permissions::NETWORK_LOOPBACK],
            untouched,
        ),
        scenario(
            "tcp_udp_loopback_perm",
            &[
                permissions::NETWORK_TCP,
                permissions::NETWORK_UDP,
                permissions::NETWORK_LOOPBACK,
            ],
            |_, o| {
                over(o);
            },
        ),
        scenario("config_loopback", &[permissions::NETWORK_UDP], |c, _| {
            c.loopback_only = true;
        }),
        scenario(
            "config_loopback_override_off",
            &[permissions::NETWORK_UDP],
            |c, o| {
                c.loopback_only = true;
                over(o).loopback_only = Some(false);
            },
        ),
        scenario(
            "override_loopback_on",
            &[permissions::NETWORK_UDP],
            |_, o| {
                over(o).loopback_only = Some(true);
            },
        ),
        scenario(
            "override_loopback_off_perm_on",
            &[permissions::NETWORK_UDP, permissions::NETWORK_LOOPBACK],
            |c, o| {
                c.loopback_only = true;
                over(o).loopback_only = Some(false);
            },
        ),
        scenario(
            "override_loopback_unset",
            &[permissions::NETWORK_UDP],
            |c, o| {
                c.loopback_only = true;
                over(o);
            },
        ),
        scenario("http_outbound", &[permissions::HTTP_OUTBOUND], untouched),
    ]
}

fn environment_scenarios() -> Vec<Scenario> {
    vec![
        scenario("config_inherit_env", &[], |c, _| c.inherit_env = true),
        scenario("sys_env", &[permissions::SYS_ENV], untouched),
        scenario("sys_env_path", &["sys.env.PATH"], untouched),
        scenario("sys_env_other", &["sys.env.NOT_A_VARIABLE"], untouched),
        scenario(
            "sys_env_prefix_only",
            &[permissions::SYS_ENV_PREFIX],
            untouched,
        ),
        scenario("sys_env_path_and_injected", &["sys.env.PATH"], |_, o| {
            over(o)
                .environment
                .insert("PROBE_INJECTED".into(), "both".into());
        }),
        scenario("inherit_env_and_injected", &[], |c, o| {
            c.inherit_env = true;
            over(o)
                .environment
                .insert("PROBE_INJECTED".into(), "both".into());
        }),
    ]
}

fn data_scenarios() -> Vec<Scenario> {
    vec![
        scenario("data_read", &[permissions::FS_READ_DATA], untouched),
        scenario("data_write", &[permissions::FS_WRITE_DATA], untouched),
        scenario(
            "data_read_write",
            &[permissions::FS_READ_DATA, permissions::FS_WRITE_DATA],
            untouched,
        ),
        scenario("data_no_permission", &[permissions::SYS_INFO], untouched),
        scenario("nodata", &[permissions::FS_READ_DATA], untouched),
        scenario("nodata_no_permission", &[], untouched),
    ]
}

fn memory_scenarios() -> Vec<Scenario> {
    fn mb(
        c: &mut PluginsConfig,
        o: &mut Option<PluginOverride>,
        config: Option<u64>,
        ov: Option<u64>,
    ) {
        c.max_memory_mb = config;
        let slot = over(o);
        slot.max_memory_mb = ov;
    }
    vec![
        scenario("memory_unlimited", &[], |c, o| mb(c, o, None, None)),
        scenario("memory_config_small", &[], |c, o| mb(c, o, Some(16), None)),
        scenario("memory_config_large", &[], |c, o| {
            mb(c, o, Some(4096), None);
        }),
        scenario("memory_override_small", &[], |c, o| {
            mb(c, o, Some(4096), Some(16));
        }),
        scenario("memory_override_large", &[], |c, o| {
            mb(c, o, Some(16), Some(4096));
        }),
        scenario("memory_override_only", &[], |c, o| mb(c, o, None, Some(16))),
        scenario("memory_config_no_override", &[], |c, _| {
            c.max_memory_mb = Some(16);
        }),
    ]
}

const WANTED: [&str; 4] = [
    permissions::NETWORK_TCP,
    permissions::NETWORK_DNS,
    permissions::FS_READ_DATA,
    "sys.env.PATH",
];

fn blocked_scenarios() -> Vec<Scenario> {
    vec![
        scenario("blocked_none", &WANTED, |_, o| {
            over(o);
        }),
        scenario("blocked_global", &WANTED, |c, o| {
            c.blocked_permissions = vec![permissions::NETWORK_TCP.into()];
            over(o);
        }),
        scenario("blocked_override", &WANTED, |_, o| {
            over(o).blocked_permissions = vec![permissions::NETWORK_DNS.into()];
        }),
        scenario("blocked_both", &WANTED, |c, o| {
            c.blocked_permissions = vec![permissions::NETWORK_TCP.into()];
            over(o).blocked_permissions = vec![permissions::FS_READ_DATA.into()];
        }),
        scenario("blocked_global_no_override", &WANTED, |c, _| {
            c.blocked_permissions = vec!["sys.env.PATH".into()];
        }),
        scenario("blocked_unrelated", &WANTED, |c, o| {
            c.blocked_permissions = vec!["network.tcp.bind".into()];
            over(o).blocked_permissions = vec!["fs.write.data".into()];
        }),
        scenario("blocked_everything", &WANTED, |c, _| {
            c.blocked_permissions = WANTED.iter().map(|p| (*p).to_owned()).collect();
        }),
    ]
}

async fn server(folder: &Path, plugins: PluginsConfig) -> Arc<Server> {
    let basic = BasicConfiguration {
        default_level_name: folder.join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    let telemetry = TelemetryConfig {
        enabled: false,
        ..TelemetryConfig::default()
    };
    let advanced = AdvancedConfiguration {
        plugins,
        ..AdvancedConfiguration::default()
    };
    Server::new(
        basic,
        advanced,
        telemetry,
        VanillaData::default(),
        Vec::new(),
    )
    .await
    .expect("a server starts on an empty folder")
}

fn describe_result(result: &Result<Result<(), String>, wasmtime::Error>) -> String {
    match result {
        Ok(Ok(())) => "ok".to_owned(),
        Ok(Err(report)) if report.contains('\n') => format!("guest report\n{report}"),
        Ok(Err(message)) => format!("guest error: {message}"),
        Err(_) => "host error".to_owned(),
    }
}

/// A scenario's server settings: the global part (the scenarios of one group that share it share
/// a server, which is the slow part to start) and the plugin's own override.
fn settings(scenario: &Scenario, port: u16) -> (PluginsConfig, Option<PluginOverride>) {
    let mut config = PluginsConfig::default();
    let mut slot = None;
    (scenario.tweak)(&mut config, &mut slot);
    if let Some(plugin_override) = slot.as_mut() {
        plugin_override
            .environment
            .insert("PROBE_PORT".into(), port.to_string());
    }
    (config, slot)
}

async fn run_scenario(
    root: &Path,
    runtime: &PluginRuntime,
    server: Arc<Server>,
    scenario: &Scenario,
) -> String {
    let manager = Arc::new(PluginManager::new(Vec::new()));
    let metadata = PluginMetadata {
        name: scenario.name.to_owned(),
        version: "1".to_owned(),
        authors: Vec::new(),
        description: String::new(),
        dependencies: Vec::new(),
        permissions: scenario.permissions.clone(),
    };
    let context = Arc::new(Context::new(
        metadata,
        Arc::clone(&server),
        Arc::clone(&manager.handlers),
        Arc::clone(&manager),
        Arc::new(OnceLock::new()),
    ));
    let (plugin, _) = runtime
        .init_plugin(root.join("plugins").join("probe.wasm"), false)
        .await
        .expect("the probe plugin loads");

    let result = on_load(&plugin, context).await;
    let state = plugin
        .store
        .call(|state| {
            (
                state.permissions.clone(),
                state.name.clone(),
                state.server.is_some(),
                state.wasi_http_hooks.allow_outbound,
            )
        })
        .await;

    let mut out = format!("== {}\n{}\n", scenario.name, describe_result(&result));
    match state {
        Ok((permissions, name, has_server, http)) => {
            writeln!(
                out,
                "state permissions={permissions:?} name={name:?} server={has_server} http_outbound={http}"
            )
            .expect("write to a String");
        }
        Err(_) => out.push_str("state unreadable\n"),
    }
    out
}

/// Loads the probe plugin once per scenario of the group and prints what `on_load` did.
/// `data_folder` groups work in the temp dir, because a plugin's data folder is relative to the
/// working directory; the other groups never read the working directory.
async fn record(scenarios: Vec<Scenario>, data_folder: bool) -> String {
    let folder = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(folder.path().join("plugins/data")).expect("plugins dir");
    std::fs::write(folder.path().join("plugins/probe.wasm"), PROBE).expect("probe plugin");
    if data_folder {
        std::env::set_current_dir(folder.path()).expect("work in the temp dir");
        // A file where the plugin's data folder should be: the data folder cannot be opened.
        std::fs::write(folder.path().join("plugins/data/nodata"), b"").expect("blocking file");
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let port = listener.local_addr().expect("listener address").port();

    let runtime = PluginRuntime::new(
        folder.path().join("plugins").join("probe.wasm"),
        LegacySyncReentry::new(),
        Arc::new(TaskScheduler::new()),
        Arc::new(TokioSpawner::new(tokio::runtime::Handle::current())),
    )
    .expect("runtime");
    // One server per distinct global config, holding the overrides of all its scenarios.
    let mut configs: BTreeMap<String, PluginsConfig> = BTreeMap::new();
    let mut keys = Vec::new();
    for scenario in &scenarios {
        let (mut config, slot) = settings(scenario, port);
        let overrides = std::mem::take(&mut config.overrides);
        let key = format!("{config:?}");
        let shared = configs.entry(key.clone()).or_insert(config);
        shared.overrides.extend(overrides);
        if let Some(plugin_override) = slot {
            shared
                .overrides
                .insert(scenario.name.to_owned(), plugin_override);
        }
        keys.push(key);
    }
    let mut servers = BTreeMap::new();
    for (key, config) in configs {
        servers.insert(key, server(folder.path(), config).await);
    }

    let mut out = String::new();
    for (scenario, key) in scenarios.iter().zip(&keys) {
        let server = Arc::clone(&servers[key]);
        out.push_str(&run_scenario(folder.path(), &runtime, server, scenario).await);
    }
    for server in servers.values() {
        server.stop();
    }
    out
}

fn check(group: &str, golden: &str, got: &str) {
    if std::env::var_os("BLESS_ON_LOAD").is_some() {
        let path = format!(
            "{}/src/on_load_golden/{group}.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::write(path, got).expect("write golden");
        return;
    }
    assert_eq!(
        got, golden,
        "on_load changed what it gives the plugin ({group})"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_load_modes_and_memory() {
    let mut scenarios = mode_scenarios();
    scenarios.extend(memory_scenarios());
    let got = record(scenarios, false).await;
    check(
        "modes_memory",
        include_str!("on_load_golden/modes_memory.txt"),
        &got,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_load_tcp() {
    let got = record(tcp_scenarios(), false).await;
    check("tcp", include_str!("on_load_golden/tcp.txt"), &got);
}

#[tokio::test(flavor = "multi_thread")]
async fn on_load_udp() {
    let got = record(udp_scenarios(), false).await;
    check("udp", include_str!("on_load_golden/udp.txt"), &got);
}

#[tokio::test(flavor = "multi_thread")]
async fn on_load_environment() {
    let got = record(environment_scenarios(), false).await;
    check(
        "environment",
        include_str!("on_load_golden/environment.txt"),
        &got,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_load_data_folder_and_blocked() {
    let mut scenarios = data_scenarios();
    scenarios.extend(blocked_scenarios());
    let got = record(scenarios, true).await;
    check(
        "data_blocked",
        include_str!("on_load_golden/data_blocked.txt"),
        &got,
    );
}
