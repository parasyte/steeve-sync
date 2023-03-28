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
use log::{debug, warn};
use notify_debouncer_mini::new_debouncer;
use notify_debouncer_mini::notify::{Error as NotifyError, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, DebouncedEvent, Debouncer};
use std::fmt::Debug;
use std::time::Duration;
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

    #[error("Save error")]
    Save(#[from] SaveError),

    #[error("File system watch error")]
    Watch(#[from] NotifyError),
}

/// The primary sync service.
pub struct Steeve {
    steam_save: SteamSave,
    xbox_save: XboxSave,
    steam_watcher: Debouncer<RecommendedWatcher>,
    xbox_watcher: Debouncer<RecommendedWatcher>,
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
        let steam_watcher = {
            let xbox_save = xbox_save.clone();
            new_debouncer(
                Duration::from_millis(500),
                None,
                move |res: DebounceEventResult| {
                    if let Ok(events) = res {
                        for event in events {
                            Self::handle_steam_event(&xbox_save, event);
                        }
                    }
                },
            )?
        };
        let xbox_watcher = {
            let steam_save = steam_save.clone();
            new_debouncer(
                Duration::from_millis(500),
                None,
                move |res: DebounceEventResult| {
                    if let Ok(events) = res {
                        for event in events {
                            Self::handle_xbox_event(&steam_save, event);
                        }
                    }
                },
            )?
        };

        let mut steeve = Self {
            steam_save,
            xbox_save,
            steam_watcher,
            xbox_watcher,
        };

        // TODO: Fix directory-not-found errors by waiting for them to be created.

        // Start watching for changes
        let path = steeve.steam_save.save_dir();
        steeve
            .steam_watcher
            .watcher()
            .watch(path, RecursiveMode::Recursive)?;

        let path = steeve.xbox_save.save_dir();
        steeve
            .xbox_watcher
            .watcher()
            .watch(path, RecursiveMode::Recursive)?;

        // TODO: Attempt initial sync

        Ok(steeve)
    }

    /// Stop watching for events.
    pub fn stop(&mut self) -> Result<(), Error> {
        self.steam_watcher
            .watcher()
            .unwatch(self.steam_save.save_dir())?;
        self.xbox_watcher
            .watcher()
            .unwatch(self.xbox_save.save_dir())?;

        Ok(())
    }

    /// Event handler for Steam save directory.
    fn handle_steam_event(xbox_save: &XboxSave, event: DebouncedEvent) {
        if SteamSave::save_file(&event.path).is_none() {
            return;
        }

        debug!("Got event for Steam path: {:?}", event.path);

        // XXX: We don't need to avoid self-updates on the Xbox event handler because we won't be
        // creating files. That's the only event the Xbox handler watches for.

        match xbox_save.copy_save(&event.path) {
            Err(SaveError::NoSave | SaveError::ModifyTime) => (),
            Err(err) => warn!("Xbox save error: {:?}", err),
            _ => (),
        }
    }

    /// Event handler for Xbox save directory.
    fn handle_xbox_event(steam_save: &SteamSave, event: DebouncedEvent) {
        if XboxSave::save_file(&event.path).is_none() {
            return;
        }

        debug!("Got event for Xbox path: {:?}", event.path);

        match steam_save.copy_save(&event.path) {
            Err(SaveError::NoSave | SaveError::ModifyTime) => (),
            Err(err) => warn!("Steam save error: {:?}", err),
            _ => (),
        }
    }
}

impl Debug for Steeve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Steeve")
            .field("steam_save", &self.steam_save)
            .field("xbox_save", &self.xbox_save)
            .field("steam_watcher", &"Debouncer<RecommendedWatcher>")
            .field("xbox_watcher", &"Debouncer<RecommendedWatcher>")
            .finish()
    }
}
