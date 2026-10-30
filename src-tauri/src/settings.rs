use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub lookup_delay_ms: u64,
    pub auto_translate: bool,
    pub launch_at_startup: bool,
    pub headword_font: String,
    pub body_font: String,
    pub source_language: String,
    pub target_language: String,
    pub ai_protocol: String,
    pub ai_base_url: String,
    pub ai_model: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            lookup_delay_ms: 240,
            auto_translate: true,
            launch_at_startup: false,
            headword_font: "Georgia".into(),
            body_font: "Segoe UI".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            ai_protocol: "auto".into(),
            ai_base_url: "https://api.deepseek.com".into(),
            ai_model: String::new(),
        }
    }
}

impl AppSettings {
    pub fn validate(&mut self) -> Result<()> {
        self.lookup_delay_ms = self.lookup_delay_ms.clamp(150, 500);
        self.headword_font =
            sanitize_font_name(&self.headword_font).unwrap_or_else(|| "Georgia".into());
        self.body_font = sanitize_font_name(&self.body_font).unwrap_or_else(|| "Segoe UI".into());
        self.ai_base_url = self.ai_base_url.trim().trim_end_matches('/').to_owned();
        self.ai_protocol = self.ai_protocol.trim().to_ascii_lowercase();
        if !matches!(self.ai_protocol.as_str(), "auto" | "openai" | "anthropic") {
            self.ai_protocol = "auto".into();
        }
        self.ai_model = self.ai_model.trim().to_owned();
        self.source_language = self.source_language.trim().to_owned();
        self.target_language = self.target_language.trim().to_owned();
        if self.source_language.is_empty() {
            self.source_language = "auto".into();
        }
        if self.target_language.is_empty() {
            self.target_language = "简体中文".into();
        }
        if !self.ai_base_url.is_empty() {
            let parsed =
                url::Url::parse(&self.ai_base_url).context("AI Base URL is not a valid URL")?;
            let is_loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
            if parsed.scheme() != "https" && !(parsed.scheme() == "http" && is_loopback) {
                anyhow::bail!("AI Base URL must use HTTPS unless it points to localhost");
            }
        }
        Ok(())
    }
}

pub fn sanitize_font_name(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(['"', '\'']);
    if trimmed.is_empty()
        || trimmed.len() > 128
        || trimmed
            .chars()
            .any(|character| character.is_control() || matches!(character, ';' | '{' | '}'))
    {
        return None;
    }
    Some(trimmed.to_owned())
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<AppSettings> {
        if !self.path.exists() {
            return Ok(AppSettings::default());
        }
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let mut settings: AppSettings = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, mut settings: AppSettings) -> Result<AppSettings> {
        settings.validate()?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_vec_pretty(&settings)?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serialized)?;
        atomic_replace(&temporary, &self.path)?;
        Ok(settings)
    }
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_product_contract() {
        let settings = AppSettings::default();
        assert_eq!(settings.lookup_delay_ms, 240);
        assert_eq!(settings.target_language, "简体中文");
        assert_eq!(settings.ai_base_url, "https://api.deepseek.com");
        assert_eq!(settings.ai_protocol, "auto");
        assert!(settings.auto_translate);
    }

    #[test]
    fn clamps_delay_and_rejects_unsafe_font_names() {
        let mut settings = AppSettings {
            lookup_delay_ms: 20,
            body_font: "Segoe UI; background:red".into(),
            ..AppSettings::default()
        };
        settings.validate().unwrap();
        assert_eq!(settings.lookup_delay_ms, 150);
        assert_eq!(settings.body_font, "Segoe UI");
    }

    #[test]
    fn rejects_insecure_non_loopback_ai_urls() {
        let mut settings = AppSettings {
            ai_base_url: "http://example.com/v1".into(),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_err());
        settings.ai_base_url = "http://127.0.0.1:8080/v1".into();
        settings.validate().unwrap();
    }

    #[test]
    fn persists_settings_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(directory.path().join("settings.json"));
        let settings = AppSettings {
            lookup_delay_ms: 200,
            ..AppSettings::default()
        };
        store.save(settings.clone()).unwrap();
        assert_eq!(store.load().unwrap(), settings);
    }
}
