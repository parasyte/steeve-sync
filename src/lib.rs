//! # Steeve Sync
//!
//! Synchronize your [Deep Rock Galactic](https://www.deeprockgalactic.com/) saves between the Xbox
//! and Steam editions.
//!
//! [`Steeve`] is a blocking service that locates the save file directories, then waits for file
//! system events on the save files. When it detects a change, it will first make a backup and then
//! copy the new save over the old. The synchronization works in both directions and prioritizes the
//! files updated most recently.
#![deny(clippy::all)]

use crate::saves::{SaveError, SteamSave, SteeveSave, XboxSave};
use directories::ProjectDirs;
use hotwatch::{Error as HotwatchError, Hotwatch};
use log::{debug, warn};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use thiserror::Error;

pub mod logger;
mod saves;

/// All the ways in which [`Steeve`] can fail.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Max backups must be > 0")]
    MaxBackups,

    #[error("Could not find home directory")]
    HomeDir,

    #[error("Save error: {0}")]
    Save(#[from] SaveError),

    #[error("Hotwatch error: {error}, context: {context}")]
    Hotwatch {
        error: HotwatchError,
        context: String,
    },
}

impl Error {
    /// Create a new `Error` from [`HotwatchError`] with contextual information.
    fn from_hotwatch(error: HotwatchError, context: impl Into<String>) -> Self {
        Self::Hotwatch {
            error,
            context: context.into(),
        }
    }
}

/// The primary sync service.
#[derive(Debug)]
pub struct Steeve {
    steam_save: SteamSave,
    xbox_save: XboxSave,
    hotwatch: Hotwatch,
}

/// Messages passed between FS event handlers
#[derive(Debug)]
enum HandlerMessage {
    /// Ignore the next event, since we are triggering it.
    IgnoreEvent,
}

impl Steeve {
    /// Create a sync service.
    ///
    /// # Errors
    ///
    /// May fail if there are any I/O errors, or if the save directories cannot be located.
    pub fn new(max_backups: usize) -> Result<Self, Error> {
        if max_backups < 1 {
            return Err(Error::MaxBackups);
        }

        // Get the path for backups
        let mut backup_dir = ProjectDirs::from("org", "KodeWerx", "SteeveSync")
            .ok_or(Error::HomeDir)?
            .data_dir()
            .to_path_buf();
        backup_dir.push("Backups");

        let steam_save = SteamSave::new(max_backups, backup_dir.clone())?;
        let xbox_save = XboxSave::new(max_backups, backup_dir)?;
        let hotwatch =
            Hotwatch::new().map_err(|err| Error::from_hotwatch(err, "Creating watcher"))?;

        let mut steeve = Self {
            steam_save,
            xbox_save,
            hotwatch,
        };

        // TODO: Fix directory-not-found errors by waiting for them to be created.

        // Start watching for changes
        let (tx, rx) = sync_channel(1);
        let xbox_save = steeve.xbox_save.clone();
        let path = steeve.steam_save.save_dir();
        steeve
            .hotwatch
            .watch(path, move |event| {
                Self::handle_steam_event(&xbox_save, event, &rx);
            })
            .map_err(|err| Error::from_hotwatch(err, format!("Steam path: {path:?}")))?;

        let steam_save = steeve.steam_save.clone();
        let path = steeve.xbox_save.save_dir();
        steeve
            .hotwatch
            .watch(path, move |event| {
                Self::handle_xbox_event(&steam_save, event, &tx);
            })
            .map_err(|err| Error::from_hotwatch(err, format!("Xbox path: {path:?}")))?;

        // TODO: Attempt initial sync

        Ok(steeve)
    }

    /// Stop watching for events.
    pub fn stop(&mut self) -> Result<(), Error> {
        self.hotwatch
            .unwatch(self.steam_save.save_dir())
            .map_err(|err| Error::from_hotwatch(err, "Stopping Steam save path watcher"))?;
        self.hotwatch
            .unwatch(self.xbox_save.save_dir())
            .map_err(|err| Error::from_hotwatch(err, "Stopping Xbox save path watcher"))?;

        Ok(())
    }

    /// Event handler for Steam save directory.
    fn handle_steam_event(
        xbox_save: &XboxSave,
        event: hotwatch::Event,
        rx: &Receiver<HandlerMessage>,
    ) {
        let path = match event {
            hotwatch::Event::Write(path) => path,
            _ => return,
        };

        if SteamSave::save_file(&path).is_none() || rx.try_recv().is_ok() {
            return;
        }

        debug!("Got event for Steam path: {:?}", path);

        // XXX: We don't need to avoid self-updates on the Xbox event handler because we won't be
        // creating files. That's the only event the Xbox handler watches for.

        match xbox_save.copy_save(&path) {
            Err(SaveError::NoSave | SaveError::ModifyTime) => (),
            Err(err) => warn!("Xbox save error: {:?}", err),
            _ => (),
        }
    }

    /// Event handler for Xbox save directory.
    fn handle_xbox_event(
        steam_save: &SteamSave,
        event: hotwatch::Event,
        tx: &SyncSender<HandlerMessage>,
    ) {
        let path = match event {
            hotwatch::Event::Create(path) => path,
            _ => return,
        };

        if XboxSave::save_file(&path).is_none() {
            return;
        }

        debug!("Got event for Xbox path: {:?}", path);

        // Notify the Steam event handler that it should ignore the next event.
        // This avoids self-update events when replacing saves.
        let _ = tx.try_send(HandlerMessage::IgnoreEvent);

        match steam_save.copy_save(&path) {
            Err(SaveError::NoSave | SaveError::ModifyTime) => (),
            Err(err) => warn!("Steam save error: {:?}", err),
            _ => (),
        }
    }
}
