//! A record of what `PluginManager::spawn_plugin_initialization` does, made before the function
//! was split (`tests/golden/plugin_spawn_init.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene spawns one or more stub plugins through the function and reads, after every join:
//! how the task ended (finished, panicked, cancelled) and whether `state_notify` woke a waiter,
//! the state map, the plugin list (index, active flag, instance and loader data), the calls the
//! plugin and its loader got (in order) and the `info`/`warn`/`error` lines the plugin module
//! logged. Scenes cover a load that succeeds (with and without permissions), one that fails (with
//! failing cleanup calls too), a panic, an aborted task (before it ran and while it waited), and
//! a plugin list that changed while `on_load` ran (cleared, or an earlier entry removed so the
//! saved index points past the end or at another plugin's entry).

use std::any::Any;
use std::fmt::{Debug, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use tokio::task::JoinHandle;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::plugin::loader::{LoaderError, PluginLoadFuture, PluginLoader, PluginUnloadFuture};
use crate::plugin::{Context, Plugin, PluginFuture, PluginManager, PluginMetadata};
use crate::server::Server;

type Lines = Arc<Mutex<Vec<String>>>;

fn push(lines: &Lines, line: String) {
    lines
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(line);
}

fn drain(lines: &Lines) -> Vec<String> {
    std::mem::take(
        &mut *lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Keeps the formatted message of every event the plugin module logs on this thread.
struct Capture(Lines);

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if !event
            .metadata()
            .target()
            .starts_with("pumpkin_core::plugin")
        {
            return;
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        push(
            &self.0,
            format!("{} {}", event.metadata().level(), message.0),
        );
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// What `on_load` does after it ran its meddling.
#[derive(Clone, Copy)]
enum Load {
    Succeed,
    Fail(&'static str),
    Panic,
    Wait,
}

/// A change to the manager's plugin list made from inside `on_load`.
#[derive(Clone, Copy)]
enum Meddle {
    Nothing,
    ClearAll,
    RemoveFirst,
}

#[derive(Clone)]
struct Spec {
    name: &'static str,
    permissions: &'static [&'static str],
    load: Load,
    meddle: Meddle,
    unload_ok: bool,
    loader_unload_ok: bool,
}

impl Spec {
    const fn new(name: &'static str, load: Load) -> Self {
        Self {
            name,
            permissions: &[],
            load,
            meddle: Meddle::Nothing,
            unload_ok: true,
            loader_unload_ok: true,
        }
    }

    const fn permissions(mut self, permissions: &'static [&'static str]) -> Self {
        self.permissions = permissions;
        self
    }

    const fn meddle(mut self, meddle: Meddle) -> Self {
        self.meddle = meddle;
        self
    }

    const fn cleanup_fails(mut self) -> Self {
        self.unload_ok = false;
        self.loader_unload_ok = false;
        self
    }
}

struct Stub {
    spec: Spec,
    manager: Arc<PluginManager>,
    calls: Lines,
}

fn describe(context: &Context) -> String {
    let metadata = context.get_metadata();
    format!("{}/{}", metadata.name, metadata.version)
}

impl Plugin for Stub {
    fn on_load(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let name = self.spec.name;
            let state = self.manager.plugin_states.read().await.get(name).cloned();
            let seen = {
                let plugins = self
                    .manager
                    .plugins
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                plugins
                    .iter()
                    .position(|plugin| Arc::ptr_eq(&plugin.context, &context))
                    .map(|index| {
                        let plugin = &plugins[index];
                        format!(
                            "entry {index} active={} instance={}",
                            plugin.is_active,
                            plugin.instance.is_some()
                        )
                    })
            };
            push(
                &self.calls,
                format!(
                    "{name}: on_load ctx={} state={state:?} {}",
                    describe(&context),
                    seen.unwrap_or_else(|| "no entry".to_owned())
                ),
            );
            {
                let mut plugins = self
                    .manager
                    .plugins
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match self.spec.meddle {
                    Meddle::Nothing => {}
                    Meddle::ClearAll => plugins.clear(),
                    Meddle::RemoveFirst => {
                        if !plugins.is_empty() {
                            plugins.remove(0);
                        }
                    }
                }
            }
            match self.spec.load {
                Load::Succeed => Ok(()),
                Load::Fail(message) => Err(message.to_owned()),
                Load::Panic => panic!("plugin {name} exploded"),
                Load::Wait => std::future::pending().await,
            }
        })
    }

    fn on_unload(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            push(
                &self.calls,
                format!("{}: on_unload ctx={}", self.spec.name, describe(&context)),
            );
            if self.spec.unload_ok {
                Ok(())
            } else {
                Err("unload failed".to_owned())
            }
        })
    }
}

struct Loader {
    ok: bool,
    calls: Lines,
}

impl PluginLoader for Loader {
    fn load<'a>(&'a self, _path: &'a Path) -> PluginLoadFuture<'a> {
        Box::pin(async { Err(LoaderError::MetadataMissing) })
    }

    fn can_load(&self, _path: &Path) -> bool {
        false
    }

    fn unload(&self, data: Box<dyn Any + Send + Sync>) -> PluginUnloadFuture<'_> {
        Box::pin(async move {
            let data = data.downcast_ref::<String>().cloned().unwrap_or_default();
            push(&self.calls, format!("loader.unload {data}"));
            if self.ok {
                Ok(())
            } else {
                Err(LoaderError::RuntimeError("unload failed".to_owned()))
            }
        })
    }

    fn can_unload(&self) -> bool {
        true
    }
}

struct Harness {
    server: Arc<Server>,
    manager: Arc<PluginManager>,
    calls: Lines,
    logs: Lines,
    instances: Vec<(&'static str, Arc<dyn Plugin>)>,
    _guard: tracing::subscriber::DefaultGuard,
}

impl Harness {
    fn new(server: &Arc<Server>) -> Self {
        let logs = Lines::default();
        let guard = tracing::subscriber::set_default(Capture(logs.clone()));
        Self {
            server: server.clone(),
            manager: Arc::new(PluginManager::new(Vec::new())),
            calls: Lines::default(),
            logs,
            instances: Vec::new(),
            _guard: guard,
        }
    }

    /// Calls the function for `spec`; the task it spawns has not run when this returns.
    async fn spawn(&mut self, spec: &Spec, out: &mut String) -> JoinHandle<()> {
        let plugin: Arc<dyn Plugin> = Arc::new(Stub {
            spec: spec.clone(),
            manager: self.manager.clone(),
            calls: self.calls.clone(),
        });
        self.instances.push((spec.name, plugin.clone()));
        let metadata = PluginMetadata {
            name: spec.name.to_owned(),
            version: "1.2".to_owned(),
            authors: Vec::new(),
            description: String::new(),
            dependencies: Vec::new(),
            permissions: spec.permissions.iter().map(|p| (*p).to_owned()).collect(),
        };
        let loader: Arc<dyn PluginLoader> = Arc::new(Loader {
            ok: spec.loader_unload_ok,
            calls: self.calls.clone(),
        });
        let data = Box::new(format!("data-{}", spec.name)) as Box<dyn Any + Send + Sync>;
        let path = PathBuf::from(format!("plugins/{}.stub", spec.name));
        let result = self
            .manager
            .spawn_plugin_initialization(self.server.clone(), plugin, metadata, data, loader, path)
            .await;
        let _ = writeln!(out, "spawn {}:", spec.name);
        match result {
            Ok(handle) => {
                let _ = writeln!(out, "  returned a task, {}", self.state_line().await);
                handle
            }
            Err(error) => panic!("spawn failed: {error}"),
        }
    }

    async fn state_line(&self) -> String {
        let states = self.manager.plugin_states.read().await;
        let mut names: Vec<_> = states.keys().collect();
        names.sort();
        let mut line = String::from("states");
        for name in names {
            let _ = write!(line, " {name}={:?}", states[name]);
        }
        line
    }

    fn plugin_lines(&self) -> String {
        let plugins = self
            .manager
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut text = format!("  plugins {}\n", plugins.len());
        for (index, plugin) in plugins.iter().enumerate() {
            let instance = plugin.instance.as_ref().map_or("none", |held| {
                self.instances
                    .iter()
                    .find(|(_, ours)| std::ptr::addr_eq(Arc::as_ptr(ours), Arc::as_ptr(held)))
                    .map_or("foreign", |(name, _)| name)
            });
            let data = plugin
                .loader_data
                .as_ref()
                .and_then(|data| data.downcast_ref::<String>())
                .map_or("none", String::as_str);
            let _ = writeln!(
                text,
                "    {index}: {} v{} active={} instance={instance} data={data} path={}",
                plugin.metadata.name,
                plugin.metadata.version,
                plugin.is_active,
                plugin.path.display()
            );
        }
        text
    }

    async fn report(&self, out: &mut String) {
        let _ = writeln!(out, "  {}", self.state_line().await);
        out.push_str(&self.plugin_lines());
        for call in drain(&self.calls) {
            let _ = writeln!(out, "  call {call}");
        }
        for log in drain(&self.logs) {
            let _ = writeln!(out, "  log {log}");
        }
    }

    /// Waits for the task, noting how it ended and whether a waiter on `state_notify` woke.
    async fn join(&self, handle: JoinHandle<()>, label: &str, out: &mut String) {
        let mut notified = Box::pin(self.manager.state_notify.notified());
        notified.as_mut().enable();
        let ended = match handle.await {
            Ok(()) => "finished",
            Err(error) if error.is_panic() => "panicked",
            Err(error) if error.is_cancelled() => "cancelled",
            Err(_) => "failed",
        };
        let woken = notified.now_or_never().is_some();
        let _ = writeln!(out, "join {label}: {ended}, waiter woken={woken}");
        self.report(out).await;
    }

    /// Lets the tasks run until the waiting ones have started.
    async fn settle(&self) {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }
}

async fn run_one(server: &Arc<Server>, title: &str, spec: Spec, out: &mut String) {
    let _ = writeln!(out, "== {title}");
    let mut harness = Harness::new(server);
    let handle = harness.spawn(&spec, out).await;
    harness.join(handle, spec.name, out).await;
}

async fn outcome_scenes(server: &Arc<Server>, out: &mut String) {
    run_one(server, "loads", Spec::new("alpha", Load::Succeed), out).await;
    run_one(
        server,
        "loads with permissions",
        Spec::new("alpha", Load::Succeed).permissions(&["fs.read", "net"]),
        out,
    )
    .await;
    run_one(server, "fails", Spec::new("alpha", Load::Fail("boom")), out).await;
    run_one(
        server,
        "fails, the cleanup calls fail too",
        Spec::new("alpha", Load::Fail("boom")).cleanup_fails(),
        out,
    )
    .await;
    run_one(
        server,
        "fails with permissions",
        Spec::new("alpha", Load::Fail("denied")).permissions(&["net"]),
        out,
    )
    .await;
    run_one(server, "panics", Spec::new("alpha", Load::Panic), out).await;
}

async fn old_state_scene(server: &Arc<Server>, out: &mut String) {
    let _ = writeln!(out, "== an old failed state is replaced by Loading");
    let mut harness = Harness::new(server);
    harness.manager.plugin_states.write().await.insert(
        "alpha".to_owned(),
        crate::plugin::PluginState::Failed("old".to_owned()),
    );
    harness
        .manager
        .plugin_states
        .write()
        .await
        .insert("other".to_owned(), crate::plugin::PluginState::Loaded);
    let spec = Spec::new("alpha", Load::Succeed);
    let handle = harness.spawn(&spec, out).await;
    harness.join(handle, "alpha", out).await;
}

async fn abort_scenes(server: &Arc<Server>, out: &mut String) {
    let _ = writeln!(out, "== aborted before it ran");
    let mut harness = Harness::new(server);
    let spec = Spec::new("alpha", Load::Succeed);
    let handle = harness.spawn(&spec, out).await;
    handle.abort();
    harness.join(handle, "alpha", out).await;

    let _ = writeln!(out, "== aborted while on_load waits");
    let mut harness = Harness::new(server);
    let spec = Spec::new("alpha", Load::Wait);
    let handle = harness.spawn(&spec, out).await;
    harness.settle().await;
    harness.report(out).await;
    handle.abort();
    harness.join(handle, "alpha", out).await;
}

async fn list_changed_scenes(server: &Arc<Server>, out: &mut String) {
    for (title, load) in [
        ("list cleared during a successful load", Load::Succeed),
        ("list cleared during a failing load", Load::Fail("boom")),
    ] {
        let _ = writeln!(out, "== {title}");
        let mut harness = Harness::new(server);
        let spec = Spec::new("alpha", load).meddle(Meddle::ClearAll);
        let handle = harness.spawn(&spec, out).await;
        harness.join(handle, "alpha", out).await;
    }
}

/// Three plugins spawned before any task runs; the second one removes the first entry from the
/// list, so its saved index points at the third plugin's entry (or past the end).
async fn shifted_scenes(server: &Arc<Server>, out: &mut String) {
    for (title, load) in [
        (
            "an earlier entry removed during a successful load",
            Load::Succeed,
        ),
        (
            "an earlier entry removed during a failing load",
            Load::Fail("boom"),
        ),
    ] {
        let _ = writeln!(out, "== {title}");
        let mut harness = Harness::new(server);
        let first = harness.spawn(&Spec::new("first", Load::Wait), out).await;
        let second = harness
            .spawn(&Spec::new("second", load).meddle(Meddle::RemoveFirst), out)
            .await;
        let third = harness.spawn(&Spec::new("third", Load::Wait), out).await;
        harness.join(second, "second", out).await;
        first.abort();
        third.abort();
        harness.join(first, "first", out).await;
        harness.join(third, "third", out).await;
    }
    let _ = writeln!(out, "== the removing plugin is the last entry");
    let mut harness = Harness::new(server);
    let first = harness.spawn(&Spec::new("first", Load::Wait), out).await;
    let second = harness
        .spawn(
            &Spec::new("second", Load::Succeed).meddle(Meddle::RemoveFirst),
            out,
        )
        .await;
    harness.join(second, "second", out).await;
    first.abort();
    harness.join(first, "first", out).await;
}

/// Plugins loaded one after the other, then spawned together with one failing in the middle.
async fn several_scenes(server: &Arc<Server>, out: &mut String) {
    let _ = writeln!(
        out,
        "== three plugins one after the other, the middle one fails"
    );
    let mut harness = Harness::new(server);
    for (name, load) in [
        ("one", Load::Succeed),
        ("two", Load::Fail("no")),
        ("three", Load::Succeed),
    ] {
        let handle = harness.spawn(&Spec::new(name, load), out).await;
        harness.join(handle, name, out).await;
    }

    let _ = writeln!(
        out,
        "== three plugins spawned together, the middle one fails"
    );
    let mut harness = Harness::new(server);
    let mut handles = Vec::new();
    for (name, load) in [
        ("one", Load::Succeed),
        ("two", Load::Fail("no")),
        ("three", Load::Succeed),
    ] {
        handles.push((name, harness.spawn(&Spec::new(name, load), out).await));
    }
    for (name, handle) in handles {
        harness.join(handle, name, out).await;
    }
}

#[tokio::test]
async fn spawn_plugin_initialization_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let mut out = String::new();
    outcome_scenes(&server, &mut out).await;
    old_state_scene(&server, &mut out).await;
    abort_scenes(&server, &mut out).await;
    list_changed_scenes(&server, &mut out).await;
    shifted_scenes(&server, &mut out).await;
    several_scenes(&server, &mut out).await;
    server.stop();
    script_golden::bless_or_compare("plugin_spawn_init.txt", &out)
}
