//! Discord Rich Presence, replacing `discordwrapper.js`.
//!
//! Presence is decorative: every failure here is logged and swallowed, because
//! a missing or closed Discord client must never interfere with launching the
//! game. The Electron version had the same property.

use std::sync::Mutex;

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

/// Holds the connection, if one was established.
#[derive(Default)]
pub struct DiscordState {
    client: Mutex<Option<DiscordIpcClient>>,
}

impl DiscordState {
    /// Connect and show the initial presence. Distribution-supplied ids and
    /// image keys mirror the `discord` blocks in the distribution index.
    pub fn initialize(&self, client_id: &str, details: &str, state_line: &str, large_key: &str, large_text: &str, small_key: &str, small_text: &str) {
        let mut guard = self.client.lock().unwrap();
        if guard.is_some() {
            return;
        }

        let mut client = match DiscordIpcClient::new(client_id) {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(%err, "Discord RPC unavailable");
                return;
            }
        };
        if let Err(err) = client.connect() {
            tracing::warn!(%err, "Discord RPC connect failed");
            return;
        }

        let activity = activity::Activity::new()
            .details(details)
            .state(state_line)
            .assets(
                activity::Assets::new()
                    .large_image(large_key)
                    .large_text(large_text)
                    .small_image(small_key)
                    .small_text(small_text),
            );

        if let Err(err) = client.set_activity(activity) {
            tracing::warn!(%err, "Failed to set Discord activity");
        } else {
            tracing::info!("Discord RPC connected");
        }
        *guard = Some(client);
    }

    /// Update just the details line, e.g. "Exploring the Realm".
    pub fn set_details(&self, details: &str, state_line: &str) {
        let mut guard = self.client.lock().unwrap();
        let Some(client) = guard.as_mut() else { return };
        let activity = activity::Activity::new().details(details).state(state_line);
        if let Err(err) = client.set_activity(activity) {
            tracing::warn!(%err, "Failed to update Discord activity");
        }
    }

    pub fn shutdown(&self) {
        let mut guard = self.client.lock().unwrap();
        if let Some(client) = guard.as_mut() {
            let _ = client.close();
        }
        *guard = None;
    }
}
