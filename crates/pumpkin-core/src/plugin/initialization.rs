//! The steps of `PluginManager::spawn_plugin_initialization`: the entry and state of a plugin
//! that is loading, and what its task does once `on_load` has answered.

use super::{
    Context, LoadedPlugin, Plugin, PluginLoader, PluginManager, PluginMetadata, PluginState,
};
use std::sync::Arc;
use tracing::{error, info, warn};

impl PluginManager {
    pub(super) async fn set_state(&self, name: &str, state: PluginState) {
        self.plugin_states
            .write()
            .await
            .insert(name.to_owned(), state);
    }

    /// Adds the entry of a plugin that is loading; returns its index in the plugin list.
    pub(super) fn register_loading(&self, plugin: LoadedPlugin) -> usize {
        let mut plugins = self
            .plugins
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plugins.push(plugin);
        plugins.len() - 1
    }

    /// Runs `on_load` of a plugin and records the outcome.
    pub(super) async fn initialize_plugin(
        &self,
        plugin_index: usize,
        instance: Arc<dyn Plugin>,
        context: Arc<Context>,
        metadata: PluginMetadata,
        loader: Arc<dyn PluginLoader>,
    ) {
        match instance.on_load(context.clone()).await {
            Ok(()) => self.finish_loaded(plugin_index, instance, &metadata).await,
            Err(e) => {
                let error_msg = format!("Initialization failed: {e}");
                let _ = instance.on_unload(context).await;
                self.discard_plugin(plugin_index, &loader).await;
                self.set_state(&metadata.name, PluginState::Failed(error_msg.clone()))
                    .await;
                self.state_notify.notify_waiters();

                error!("Failed to initialize plugin {}: {error_msg}", metadata.name);
            }
        }
    }

    async fn finish_loaded(
        &self,
        plugin_index: usize,
        instance: Arc<dyn Plugin>,
        metadata: &PluginMetadata,
    ) {
        {
            let mut plugins = self
                .plugins
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(plugin) = plugins.get_mut(plugin_index) {
                plugin.instance = Some(instance);
                plugin.is_active = true;
            }
        }
        self.set_state(&metadata.name, PluginState::Loaded).await;
        self.state_notify.notify_waiters();

        info!("Loaded {} ({})", metadata.name, metadata.version);

        if !metadata.permissions.is_empty() {
            warn!(
                "Plugin \"{}\" uses the following permissions: {:?}",
                metadata.name, metadata.permissions
            );
        }
    }

    /// Hands the loader data of a plugin that failed to load back to its loader and removes the entry.
    async fn discard_plugin(&self, plugin_index: usize, loader: &Arc<dyn PluginLoader>) {
        let loader_data = self
            .plugins
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(plugin_index)
            .and_then(|plugin| plugin.loader_data.take());

        if let Some(data) = loader_data {
            loader.unload(data).await.ok();
        }

        let mut plugins = self
            .plugins
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if plugin_index < plugins.len() {
            plugins.remove(plugin_index);
        }
    }
}
