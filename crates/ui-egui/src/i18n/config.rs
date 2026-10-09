//! Versioned language manifest shared by the bundler, runtime and maintenance tooling.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PACK_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_LANGUAGES: usize = 64;
pub const MAX_CODE_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluralRule {
    None,
    OneOther,
    French,
    Portuguese,
    Russian,
    Czech,
}

impl PluralRule {
    pub fn index(self, n: u64) -> usize {
        match self {
            Self::None => 0,
            Self::OneOther => usize::from(n != 1),
            Self::French | Self::Portuguese => usize::from(n > 1),
            Self::Russian => match (n % 10, n % 100) {
                (1..=4, 11..=19) => 2,
                (1, _) => 0,
                (2..=4, _) => 1,
                _ => 2,
            },
            Self::Czech => match n {
                1 => 0,
                2..=4 => 1,
                _ => 2,
            },
        }
    }

    pub fn forms(self) -> usize {
        match self {
            Self::None => 1,
            Self::OneOther | Self::French | Self::Portuguese => 2,
            Self::Russian | Self::Czech => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanguageConfig {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub catalog: Option<String>,
    pub plural_rule: PluralRule,
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub complete_menus: bool,
}

impl LanguageConfig {
    pub fn fallback<'a>(&'a self, default: &'a str) -> Option<&'a str> {
        self.fallback.as_deref().or_else(|| {
            if self.code == "en" {
                None
            } else if self.code == default {
                Some("en")
            } else {
                Some(default)
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub schema_version: u32,
    #[serde(default = "english")]
    pub fallback_locale: String,
    #[serde(default = "reload_interval")]
    pub reload_interval_ms: u64,
    pub languages: Vec<LanguageConfig>,
}

fn english() -> String {
    "en".into()
}
fn reload_interval() -> u64 {
    1000
}

pub fn valid_code(code: &str) -> bool {
    if matches!(code, "auto" | "posix") || code.is_empty() || code.len() > MAX_CODE_BYTES || code != code.to_ascii_lowercase() {
        return false;
    }
    let mut parts = code.split('-');
    parts.next().is_some_and(|p| (2..=8).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_lowercase()))
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

/// Flat, portable file names make resource locations reviewable and prevent path traversal.
pub fn valid_catalog_name(name: &str) -> bool {
    if name.len() > 80 {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || stem.strip_prefix("com").or_else(|| stem.strip_prefix("lpt")).is_some_and(|s| s.len() == 1 && matches!(s.as_bytes().first(), Some(b'1'..=b'9')));
    !reserved
        && name.len() <= 80
        && name.ends_with(".tsv")
        && !name.starts_with('.')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err("manifest exceeds 64 KiB".into());
        }
        let manifest: Self = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("manifest: {e}"))?;
        manifest.validate_fields()?;
        Ok(manifest)
    }

    pub fn validate_fields(&self) -> Result<(), String> {
        if self.schema.as_ref().is_some_and(|s| s.len() > 256) {
            return Err("$schema is too long".into());
        }
        if self.schema_version != 1 {
            return Err(format!("unsupported localisation schema version {} (expected 1)", self.schema_version));
        }
        if !(250..=60_000).contains(&self.reload_interval_ms) {
            return Err("reloadIntervalMs must be between 250 and 60000".into());
        }
        if self.languages.is_empty() || self.languages.len() > MAX_LANGUAGES {
            return Err("manifest must contain 1 to 64 languages".into());
        }
        if !valid_code(&self.fallback_locale) {
            return Err("invalid fallbackLocale".into());
        }
        let mut codes = BTreeSet::new();
        let mut files = BTreeSet::new();
        for language in &self.languages {
            if !valid_code(&language.code) || !codes.insert(language.code.as_str()) {
                return Err(format!("invalid or duplicate language code {:?}", language.code));
            }
            if language.name.trim().is_empty() || language.name.len() > 128 || language.name.chars().any(char::is_control) {
                return Err(format!("{}: name must be nonempty text of at most 128 bytes", language.code));
            }
            if let Some(file) = &language.catalog
                && (!valid_catalog_name(file) || !files.insert(file.as_str()))
            {
                return Err(format!("{}: invalid or duplicate catalog file {file:?}", language.code));
            }
            if language.fallback.as_deref().is_some_and(|code| !valid_code(code)) {
                return Err(format!("{}: invalid fallback code", language.code));
            }
            if language.aliases.len() > 32 || language.aliases.iter().any(|code| !valid_code(code)) {
                return Err(format!("{}: aliases must contain at most 32 valid locale tags", language.code));
            }
        }
        // All fields have bounded sizes before serialisation, including control-channel data.
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_MANIFEST_BYTES {
            return Err("manifest exceeds 64 KiB".into());
        }
        Ok(())
    }

    /// Validate the merged registry, including references and cycles, before publishing it.
    pub fn validate_registry(&self, languages: &[LanguageConfig]) -> Result<(), String> {
        if languages.len() > MAX_LANGUAGES {
            return Err("merged registry exceeds 64 languages".into());
        }
        let by_code: BTreeMap<_, _> = languages.iter().map(|l| (l.code.as_str(), l)).collect();
        if by_code.len() != languages.len() {
            return Err("duplicate language in merged registry".into());
        }
        if !by_code.contains_key("en") || !by_code.contains_key(self.fallback_locale.as_str()) {
            return Err("registry must contain English and fallbackLocale".into());
        }
        let mut aliases = BTreeMap::new();
        for language in languages {
            for alias in &language.aliases {
                if (by_code.contains_key(alias.as_str()) && alias != &language.code) || aliases.insert(alias.as_str(), language.code.as_str()).is_some() {
                    return Err(format!("duplicate or ambiguous alias {alias:?}"));
                }
            }
            let mut seen = BTreeSet::new();
            let mut current = Some(language.code.as_str());
            while let Some(code) = current {
                if !seen.insert(code) {
                    return Err(format!("{}: fallback cycle at {code}", language.code));
                }
                let entry = by_code.get(code).ok_or_else(|| format!("{}: unknown fallback {code}", language.code))?;
                current = entry.fallback(&self.fallback_locale);
            }
        }
        Ok(())
    }
}
