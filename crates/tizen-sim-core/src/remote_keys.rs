//! Remote Keys: the TV remote's buttons, and how the desktop keyboard maps
//! onto them.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A button on the TV remote.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteKey {
    /// The Tizen key name, e.g. `ColorF0Red`.
    pub name: String,
    /// The `keyCode` Apps receive.
    pub code: u16,
    /// Mandatory Keys are always delivered; the rest must be registered.
    #[serde(default)]
    pub mandatory: bool,
    /// `KeyboardEvent.key`, for keys with a standard value.
    pub dom_key: Option<String>,
}

#[derive(Deserialize)]
struct Catalog {
    key: Vec<RemoteKey>,
}

fn catalog() -> &'static [RemoteKey] {
    static CATALOG: OnceLock<Vec<RemoteKey>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str::<Catalog>(include_str!("../data/remote_keys.toml"))
            .expect("bundled remote_keys.toml is valid")
            .key
    })
}

impl RemoteKey {
    pub fn find(name: &str) -> Option<&'static RemoteKey> {
        catalog().iter().find(|k| k.name == name)
    }

    pub fn all() -> &'static [RemoteKey] {
        catalog()
    }
}

/// A key on the desktop keyboard, identified by its Windows virtual key code
/// (`KeyboardEvent.keyCode`), which every Engine down to M47 reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicalKey(pub u16);

#[derive(Debug, thiserror::Error)]
#[error("unknown keyboard key `{0}` (use a name like F5, Backspace or A, or a key code)")]
pub struct UnknownPhysicalKey(pub String);

#[derive(Debug, thiserror::Error)]
#[error("unknown Remote Key `{0}`")]
pub struct UnknownRemoteKey(pub String);

const NAMED_KEYS: &[(&str, u16)] = &[
    ("Backspace", 8),
    ("Tab", 9),
    ("Enter", 13),
    ("Escape", 27),
    ("Esc", 27),
    ("Space", 32),
    ("PageUp", 33),
    ("PageDown", 34),
    ("End", 35),
    ("Home", 36),
    ("ArrowLeft", 37),
    ("ArrowUp", 38),
    ("ArrowRight", 39),
    ("ArrowDown", 40),
    ("Insert", 45),
    ("Delete", 46),
    ("MediaTrackNext", 176),
    ("MediaTrackPrevious", 177),
    ("MediaStop", 178),
    ("MediaPlayPause", 179),
];

impl PhysicalKey {
    /// Parses `F5`, `Backspace`, `A`, `7` or a raw key code such as `116`.
    pub fn parse(text: &str) -> Result<PhysicalKey, UnknownPhysicalKey> {
        let unknown = || UnknownPhysicalKey(text.to_string());
        if let Some(&(_, code)) = NAMED_KEYS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(text))
        {
            return Ok(PhysicalKey(code));
        }
        if let Some(n) = text
            .strip_prefix(['F', 'f'])
            .and_then(|n| n.parse::<u16>().ok())
            .filter(|n| (1..=12).contains(n))
        {
            return Ok(PhysicalKey(111 + n));
        }
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && c.is_ascii_alphanumeric()
        {
            return Ok(PhysicalKey(c.to_ascii_uppercase() as u16));
        }
        text.parse::<u16>().map(PhysicalKey).map_err(|_| unknown())
    }

    /// The `KeyboardEvent.key` this key produces, where it has a fixed one.
    pub(crate) fn dom_key(self) -> Option<String> {
        if let Some(&(name, _)) = NAMED_KEYS.iter().find(|&&(_, code)| code == self.0) {
            return Some(name.to_string());
        }
        match self.0 {
            112..=123 => Some(format!("F{}", self.0 - 111)),
            48..=57 | 65..=90 => Some(char::from(self.0 as u8).to_string()),
            _ => None,
        }
    }
}

/// Which keyboard keys stand in for which Remote Keys. Keys that already
/// produce the right `keyCode` (arrows, Enter, digits) need no entry.
#[derive(Debug, Clone)]
pub struct KeyMap {
    entries: BTreeMap<PhysicalKey, String>,
}

impl Default for KeyMap {
    fn default() -> KeyMap {
        let mut map = KeyMap {
            entries: BTreeMap::new(),
        };
        for (physical, remote) in [
            ("Backspace", "Back"),
            ("Escape", "Back"),
            ("F1", "ColorF0Red"),
            ("F2", "ColorF1Green"),
            ("F3", "ColorF2Yellow"),
            ("F4", "ColorF3Blue"),
            ("PageUp", "ChannelUp"),
            ("PageDown", "ChannelDown"),
            ("MediaPlayPause", "MediaPlayPause"),
            ("MediaStop", "MediaStop"),
            ("MediaTrackNext", "MediaTrackNext"),
            ("MediaTrackPrevious", "MediaTrackPrevious"),
        ] {
            map.insert(PhysicalKey::parse(physical).expect("valid default"), remote)
                .expect("default maps to a catalogued Remote Key");
        }
        map
    }
}

impl KeyMap {
    /// Maps a keyboard key to a Remote Key, replacing any earlier mapping.
    pub fn insert(
        &mut self,
        physical: PhysicalKey,
        remote_key: &str,
    ) -> Result<(), UnknownRemoteKey> {
        let key =
            RemoteKey::find(remote_key).ok_or_else(|| UnknownRemoteKey(remote_key.to_string()))?;
        self.entries.insert(physical, key.name.clone());
        Ok(())
    }

    pub fn get(&self, physical: PhysicalKey) -> Option<&str> {
        self.entries.get(&physical).map(String::as_str)
    }

    /// `{ "<keyCode>": "<Remote Key name>" }`, as the Shims read it.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        self.entries
            .iter()
            .map(|(k, v)| (k.0.to_string(), serde_json::Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::VersionProfile;

    #[test]
    fn every_profile_key_and_default_mapping_exists_in_the_catalog() {
        for profile in VersionProfile::all() {
            for name in &profile.registrable_keys {
                let key = RemoteKey::find(name).unwrap_or_else(|| {
                    panic!("profile {} lists unknown key {name}", profile.version)
                });
                assert!(
                    !key.mandatory,
                    "{name} is mandatory; profiles list Registrable Keys only"
                );
            }
        }
        for remote in KeyMap::default().entries.values() {
            assert!(
                RemoteKey::find(remote).is_some(),
                "default map targets unknown key {remote}"
            );
        }
    }

    #[test]
    fn parses_keyboard_keys_by_name_letter_digit_or_code() {
        assert_eq!(PhysicalKey::parse("Backspace").unwrap(), PhysicalKey(8));
        assert_eq!(PhysicalKey::parse("esc").unwrap(), PhysicalKey(27));
        assert_eq!(PhysicalKey::parse("F5").unwrap(), PhysicalKey(116));
        assert_eq!(PhysicalKey::parse("f12").unwrap(), PhysicalKey(123));
        assert_eq!(PhysicalKey::parse("r").unwrap(), PhysicalKey(82));
        assert_eq!(PhysicalKey::parse("7").unwrap(), PhysicalKey(55));
        assert_eq!(PhysicalKey::parse("179").unwrap(), PhysicalKey(179));
        assert!(PhysicalKey::parse("F13").is_err());
        assert!(PhysicalKey::parse("Hyper").is_err());
    }
}
