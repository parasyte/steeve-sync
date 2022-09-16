use directories::BaseDirs;
use log::{debug, info};
use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    path::{Path, PathBuf},
    time::SystemTime,
};
use steamlocate::SteamDir;
use thiserror::Error;
use walkdir::WalkDir;

/// Steam app ID for Deep Rock Galactic.
/// See: https://steamdb.info/app/548430/
const DRG_APP_ID: &u32 = &548430;

/// All the ways in which save file and backup handling can fail.
#[derive(Debug, Error)]
pub enum SaveError {
    #[error("Could not find home directory")]
    HomeDir,

    #[error("Could not find Steam")]
    SteamDir,

    #[error("Could not find Deep Rock Galactic on Steam")]
    SteamApp,

    #[error("Unable to create directory: {0}")]
    DirCreate(PathBuf),

    #[error("No save file")]
    NoSave,

    #[error("Destination was modified more recently than source")]
    ModifyTime,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Manages Steam directories for saves and backups.
#[derive(Clone, Debug)]
pub(crate) struct SteamSave {
    max_backups: usize,
    backup_dir: PathBuf,
    save_dir: PathBuf,
}

impl SteamSave {
    pub(crate) fn new(max_backups: usize, mut backup_dir: PathBuf) -> Result<Self, SaveError> {
        // Get the save path for Steam
        let mut save_dir = SteamDir::locate()
            .ok_or(SaveError::SteamDir)?
            .app(DRG_APP_ID)
            .ok_or(SaveError::SteamApp)?
            .path
            .clone();
        save_dir.push("FSD");
        save_dir.push("Saved");
        save_dir.push("SaveGames");

        backup_dir.push("Steam");

        // Create backup path
        std::fs::create_dir_all(&backup_dir)
            .map_err(|_| SaveError::DirCreate(backup_dir.clone()))?;

        Ok(Self {
            max_backups,
            save_dir,
            backup_dir,
        })
    }
}

/// Manages Xbox directories for saves and backups.
#[derive(Clone, Debug)]
pub(crate) struct XboxSave {
    max_backups: usize,
    backup_dir: PathBuf,
    save_dir: PathBuf,
}

impl XboxSave {
    pub(crate) fn new(max_backups: usize, mut backup_dir: PathBuf) -> Result<Self, SaveError> {
        // Get the save path for Xbox
        let mut save_dir = BaseDirs::new()
            .ok_or(SaveError::HomeDir)?
            .data_local_dir()
            .to_path_buf();
        save_dir.push("Packages");
        save_dir.push("CoffeeStainStudios.DeepRockGalactic_496a1srhmar9w");
        save_dir.push("SystemAppData");
        save_dir.push("wgs");
        save_dir.push("000901F266032D3B_882901006F2042808DB0569531F199CB");

        backup_dir.push("Xbox");

        // Create backup path
        std::fs::create_dir_all(&backup_dir)
            .map_err(|_| SaveError::DirCreate(backup_dir.clone()))?;

        Ok(Self {
            max_backups,
            save_dir,
            backup_dir,
        })
    }
}

/// A handy internal trait for keeping save directory handling DRY.
pub(crate) trait SteeveSave {
    /// Get the implementation name.
    fn name(&self) -> &str;

    /// Get maximum number of backups to retain.
    fn max_backups(&self) -> usize;

    /// Get the backup directory.
    fn backup_dir(&self) -> &Path;

    /// Get the save directory.
    fn save_dir(&self) -> &Path;

    /// Get the file (leaf) name if the path looks like the current save file.
    fn save_file<P: AsRef<Path>>(path: P) -> Option<String>;

    /// Copy the given save file to one that we can locate.
    fn copy_save<P: AsRef<Path>>(&self, from: P) -> Result<(), SaveError> {
        let from = from.as_ref();

        let (to, filename) = match self.locate_save_path() {
            Some((path, filename)) => (path, filename),
            None => return Err(SaveError::NoSave),
        };

        // Compare the file modify times
        let from_time = from.metadata()?.modified()?;
        let to_time = to.metadata()?.modified()?;
        if from_time <= to_time {
            return Err(SaveError::ModifyTime);
        }

        // Backup the destination save file
        self.backup(&to, &filename)?;

        // Do the final copy
        info!("Steeve is syncing a new save to {}", self.name());
        debug!("Copy {} save: {:?} -> {:?}", self.name(), from, to);
        std::fs::copy(from, &to)?;

        Ok(())
    }

    /// Find a file in the save directory that looks like the current save file.
    fn locate_save_path(&self) -> Option<(PathBuf, String)> {
        WalkDir::new(self.save_dir())
            .into_iter()
            .filter_map(|result| result.ok())
            .find_map(|entry| {
                Self::save_file(entry.path()).map(|filename| (entry.path().to_path_buf(), filename))
            })
    }

    /// Backup the save file.
    fn backup<P: AsRef<Path>>(&self, save_path: P, filename: &str) -> Result<bool, SaveError> {
        let save_path = save_path.as_ref();

        if self.is_dupe_backup(save_path)? {
            debug!("{} save backup de-duped: {:?}", self.name(), save_path);
            return Ok(false);
        }

        self.remove_old_backups()?;

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut backup_path = self.backup_dir().to_path_buf();
        backup_path.push(format!("{}_{}", timestamp, filename));

        debug!(
            "Backup {} save: {:?} -> {:?}",
            self.name(),
            save_path,
            backup_path,
        );
        std::fs::copy(save_path, backup_path)?;

        Ok(true)
    }

    /// Check if the file is already backed up.
    fn is_dupe_backup<P: AsRef<Path>>(&self, save_path: P) -> Result<bool, SaveError> {
        let save_path = save_path.as_ref();

        // File comparison is done by hashing its contents
        let bytes = std::fs::read(save_path)?;
        let mut hasher = DefaultHasher::new();
        hasher.write(&bytes);
        let save_hash = hasher.finish();

        let is_dupe = WalkDir::new(self.backup_dir())
            .into_iter()
            .filter_map(|result| result.ok())
            .any(|entry| {
                if !entry.file_type().is_file() {
                    return false;
                }

                let bytes = match std::fs::read(entry.path()) {
                    Ok(bytes) => bytes,
                    Err(_) => return false,
                };
                let mut hasher = DefaultHasher::new();
                hasher.write(&bytes);

                hasher.finish() == save_hash
            });

        Ok(is_dupe)
    }

    /// Remove old backups.
    fn remove_old_backups(&self) -> Result<(), SaveError> {
        let files = WalkDir::new(self.backup_dir())
            .sort_by_key(|entry| match entry.metadata() {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => mtime,
                    Err(_) => SystemTime::UNIX_EPOCH,
                },
                Err(_) => SystemTime::UNIX_EPOCH,
            })
            .into_iter()
            .filter_map(|result| result.ok())
            .filter(|entry| entry.file_type().is_file())
            .collect::<Vec<_>>();

        let max_backups = self.max_backups() - 1;
        if files.len() > max_backups {
            for entry in files.iter().take(files.len() - max_backups) {
                let path = entry.path();
                debug!("Removing old {} backup: {:?}", self.name(), path);
                std::fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}

impl SteeveSave for SteamSave {
    fn name(&self) -> &str {
        "Steam"
    }

    fn max_backups(&self) -> usize {
        self.max_backups
    }

    fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    fn save_dir(&self) -> &Path {
        &self.save_dir
    }

    fn save_file<P: AsRef<Path>>(path: P) -> Option<String> {
        let path = path.as_ref();
        let filename = match (path.is_file(), path.file_name()) {
            (true, Some(filename)) => filename.to_string_lossy().to_string(),
            _ => return None,
        };

        if filename.ends_with("_Player.sav") {
            Some(filename)
        } else {
            None
        }
    }
}

impl SteeveSave for XboxSave {
    fn name(&self) -> &str {
        "Xbox"
    }

    fn max_backups(&self) -> usize {
        self.max_backups
    }

    fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    fn save_dir(&self) -> &Path {
        &self.save_dir
    }

    fn save_file<P: AsRef<Path>>(path: P) -> Option<String> {
        let path = path.as_ref();
        let filename = match (path.is_file(), path.file_name()) {
            (true, Some(filename)) => filename.to_string_lossy().to_string(),
            _ => return None,
        };

        if filename.len() == 32 && filename.chars().all(|ch| ch.is_ascii_hexdigit()) {
            Some(filename)
        } else {
            None
        }
    }
}
