//! Reads and writes `config.toml`.
//!
//! Reads deserialize into [`Config`]; writes edit the document in place with
//! `toml_edit`, so comments and key order a user added by hand survive `config set`.
//! The file knows nothing about secrets: those are references resolved by
//! [`crate::secrets`].

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use paloma_core::{Error, Result};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::profile::{Profile, ProfileKey};

pub const DEFAULT_PROFILE: &str = "default";

/// The parsed config file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: IndexMap<String, Profile>,
}

impl Config {
    /// The profile marked default, or `"default"`.
    pub fn default_profile_name(&self) -> &str {
        self.default_profile
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_PROFILE)
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }
}

/// A config file at a known path.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    /// Open the config at `path` (from `--config` / `PALOMA_CONFIG`), or the platform
    /// default. When falling back to the default and only the v1 `~/.paloma/config.toml`
    /// exists, it is copied over first and the returned note says so; keyring references
    /// are unchanged, so the migrated profiles keep working.
    pub fn open(path: Option<&Path>) -> Result<(Self, Option<String>)> {
        if let Some(p) = path {
            return Ok((
                Self {
                    path: p.to_path_buf(),
                },
                None,
            ));
        }
        let path = default_path()?;
        let mut note = None;
        if !path.exists()
            && let Some(legacy) = legacy_path().filter(|p| p.is_file())
        {
            {
                create_parent(&path)?;
                fs::copy(&legacy, &path).map_err(|e| {
                    Error::io(
                        format!("cannot migrate {} to {}", legacy.display(), path.display()),
                        e,
                    )
                })?;
                note = Some(format!(
                    "migrated config from {} to {}",
                    legacy.display(),
                    path.display()
                ));
            }
        }
        Ok((Self { path }, note))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Parse the file; a missing file is an empty config.
    pub fn load(&self) -> Result<Config> {
        match self.read_text()? {
            None => Ok(Config::default()),
            Some(text) => toml::from_str(&text).map_err(|e| {
                Error::other(format!(
                    "config parse error in {}: {}",
                    self.path.display(),
                    e.message()
                ))
            }),
        }
    }

    /// Set (or, with `None`, remove) one key of a profile, creating the profile if needed.
    pub fn set_key(&self, profile: &str, key: ProfileKey, val: Option<&str>) -> Result<()> {
        self.edit(|doc| {
            let table = profile_table(doc, profile);
            match val.filter(|v| !v.is_empty()) {
                Some(v) => {
                    table[key.as_str()] = value(v);
                }
                None => {
                    table.remove(key.as_str());
                }
            }
        })
    }

    /// Insert or replace a whole profile, optionally making it the default.
    pub fn upsert_profile(&self, name: &str, profile: &Profile, make_default: bool) -> Result<()> {
        self.edit(|doc| {
            if make_default {
                doc["default_profile"] = value(name);
            }
            let table = profile_table(doc, name);
            for key in ProfileKey::ALL {
                match profile.get(key).filter(|v| !v.is_empty()) {
                    Some(v) => table[key.as_str()] = value(v),
                    None => {
                        table.remove(key.as_str());
                    }
                }
            }
        })
    }

    pub fn set_default_profile(&self, name: &str) -> Result<()> {
        self.edit(|doc| doc["default_profile"] = value(name))
    }

    fn edit(&self, f: impl FnOnce(&mut DocumentMut)) -> Result<()> {
        let mut doc = match self.read_text()? {
            Some(text) => text.parse::<DocumentMut>().map_err(|e| {
                Error::other(format!(
                    "config parse error in {}: {}",
                    self.path.display(),
                    e.message()
                ))
            })?,
            None => DocumentMut::new(),
        };
        if doc.get("default_profile").is_none() {
            doc["default_profile"] = value(DEFAULT_PROFILE);
        }
        f(&mut doc);
        create_parent(&self.path)?;
        fs::write(&self.path, doc.to_string())
            .map_err(|e| Error::io(format!("cannot write config {}", self.path.display()), e))
    }

    fn read_text(&self) -> Result<Option<String>> {
        match fs::read_to_string(&self.path) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(
                format!("cannot read config {}", self.path.display()),
                e,
            )),
        }
    }
}

/// `[profiles.<name>]`, created as needed, with `profiles` itself kept implicit so the
/// file reads as a flat list of `[profiles.x]` headers.
fn profile_table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    let profiles = doc
        .entry("profiles")
        .or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut()
        .expect("`profiles` is a table");
    profiles
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .expect("profile is a table")
}

fn create_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fs::create_dir_all(dir)
            .map_err(|e| Error::io(format!("cannot create {}", dir.display()), e)),
        _ => Ok(()),
    }
}

/// `$XDG_CONFIG_HOME/paloma/config.toml` (`~/.config/...` on Linux and macOS),
/// `%APPDATA%\paloma\config.toml` on Windows.
pub fn default_path() -> Result<PathBuf> {
    use etcetera::BaseStrategy;
    let strategy = etcetera::choose_base_strategy()
        .map_err(|e| Error::other(format!("cannot locate the config directory: {e}")))?;
    Ok(strategy.config_dir().join("paloma").join("config.toml"))
}

/// Where v1 (the Java build) kept its config.
fn legacy_path() -> Option<PathBuf> {
    etcetera::home_dir()
        .ok()
        .map(|h| h.join(".paloma").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ConfigFile::at(dir.path().join("nope.toml")).load().unwrap();
        assert_eq!(cfg.default_profile_name(), "default");
        assert!(cfg.profiles.is_empty());
    }

    #[test]
    fn edits_preserve_comments_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# my settings\ndefault_profile = \"work\"\n\n[profiles.work]\n# the main number\nphone_number_id = \"111\"\n",
        )
        .unwrap();
        let file = ConfigFile::at(&path);
        file.set_key("work", ProfileKey::WabaId, Some("222"))
            .unwrap();
        file.set_key("staging", ProfileKey::ApiVersion, Some("v26.0"))
            .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"));
        assert!(text.contains("# the main number"));
        assert!(text.contains("[profiles.staging]"));
        assert!(
            !text.contains("[profiles]\n"),
            "profiles table stays implicit:\n{text}"
        );

        let cfg = file.load().unwrap();
        assert_eq!(cfg.default_profile_name(), "work");
        assert_eq!(cfg.profile("work").unwrap().waba_id.as_deref(), Some("222"));
        assert_eq!(
            cfg.profile("staging").unwrap().api_version.as_deref(),
            Some("v26.0")
        );
    }

    #[test]
    fn upsert_sets_default_and_clears_unset_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file = ConfigFile::at(dir.path().join("sub/config.toml"));
        let p = Profile {
            phone_number_id: Some("1".into()),
            waba_id: Some("2".into()),
            ..Profile::default()
        };
        file.upsert_profile("a", &p, true).unwrap();
        let p2 = Profile {
            phone_number_id: Some("3".into()),
            ..Profile::default()
        };
        file.upsert_profile("a", &p2, false).unwrap();
        let cfg = file.load().unwrap();
        assert_eq!(cfg.default_profile_name(), "a");
        assert_eq!(cfg.profile("a").unwrap(), &p2);
    }

    #[test]
    fn parse_error_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "default_profile = ").unwrap();
        let err = ConfigFile::at(&path).load().unwrap_err().to_string();
        assert!(err.contains("bad.toml"), "{err}");
    }
}
