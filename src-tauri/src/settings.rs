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
    pub word_card_linger_ms: u64,
    #[serde(alias = "overlayOffsetPx")]
    pub overlay_offset_x_px: u64,
    pub overlay_offset_y_px: u64,
    pub ai_translation_enabled: bool,
    pub auto_translate: bool,
    pub launch_at_startup: bool,
    pub headword_font: String,
    pub body_font: String,
    pub source_language: String,
    pub target_language: String,
    pub ai_protocol: String,
    pub ai_connection_name: String,
    pub ai_base_url: String,
    pub ai_model: String,
    pub ai_roles: AiRoles,
    pub unsupported_reasoning_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiRole {
    pub connection_id: String,
    pub model: String,
    pub effort: String,
}

impl Default for AiRole {
    fn default() -> Self {
        Self {
            connection_id: "api".into(),
            model: String::new(),
            effort: "medium".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiRoles {
    pub chat: AiRole,
    pub translation: AiRole,
    pub image: AiRole,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            lookup_delay_ms: 240,
            word_card_linger_ms: 1500,
            overlay_offset_x_px: 20,
            overlay_offset_y_px: 20,
            ai_translation_enabled: true,
            auto_translate: true,
            launch_at_startup: false,
            headword_font: "Georgia".into(),
            body_font: "Segoe UI".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            ai_protocol: "auto".into(),
            ai_connection_name: "API".into(),
            ai_base_url: "https://api.deepseek.com".into(),
            ai_model: String::new(),
            ai_roles: AiRoles::default(),
            unsupported_reasoning_models: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn validate(&mut self) -> Result<()> {
        self.lookup_delay_ms = self.lookup_delay_ms.clamp(100, 500);
        self.word_card_linger_ms = self.word_card_linger_ms.clamp(0, 5000);
        self.overlay_offset_x_px = self.overlay_offset_x_px.clamp(0, 240);
        self.overlay_offset_y_px = self.overlay_offset_y_px.clamp(0, 240);
        self.headword_font =
            sanitize_font_name(&self.headword_font).unwrap_or_else(|| "Georgia".into());
        self.body_font = sanitize_font_name(&self.body_font).unwrap_or_else(|| "Segoe UI".into());
        self.ai_base_url = self.ai_base_url.trim().trim_end_matches('/').to_owned();
        self.ai_protocol = self.ai_protocol.trim().to_ascii_lowercase();
        if !matches!(self.ai_protocol.as_str(), "auto" | "openai" | "anthropic") {
            self.ai_protocol = "auto".into();
        }
        self.ai_connection_name = self.ai_connection_name.trim().to_owned();
        if self.ai_connection_name.is_empty() {
            self.ai_connection_name = "API".into();
        }
        self.ai_model = self.ai_model.trim().to_owned();
        self.unsupported_reasoning_models = self
            .unsupported_reasoning_models
            .drain(..)
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty())
            .collect();
        self.unsupported_reasoning_models.sort();
        self.unsupported_reasoning_models.dedup();
        normalize_ai_role(&mut self.ai_roles.chat, &self.ai_model);
        normalize_ai_role(&mut self.ai_roles.translation, &self.ai_model);
        normalize_ai_role(&mut self.ai_roles.image, &self.ai_model);
        if self.ai_model.is_empty() {
            self.ai_model = self.ai_roles.translation.model.clone();
        }
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

    pub fn translation_model(&self) -> &str {
        let role_model = self.ai_roles.translation.model.trim();
        if role_model.is_empty() {
            self.ai_model.trim()
        } else {
            role_model
        }
    }

    pub fn translation_effort(&self) -> &str {
        match self.ai_roles.translation.effort.as_str() {
            "low" => "low",
            "high" => "high",
            _ => "high",
        }
    }

    pub fn supports_reasoning_effort(&self, model: &str) -> bool {
        !self
            .unsupported_reasoning_models
            .iter()
            .any(|unsupported| unsupported == model)
    }

    pub fn remember_unsupported_reasoning_model(&mut self, model: &str) -> bool {
        let model = model.trim();
        if model.is_empty()
            || self
                .unsupported_reasoning_models
                .iter()
                .any(|unsupported| unsupported == model)
        {
            return false;
        }
        self.unsupported_reasoning_models.push(model.to_owned());
        self.unsupported_reasoning_models.sort();
        true
    }
}

fn normalize_ai_role(role: &mut AiRole, fallback_model: &str) {
    if role.connection_id.trim().is_empty() {
        role.connection_id = "api".into();
    }
    role.model = role.model.trim().to_owned();
    if role.model.is_empty() {
        role.model = fallback_model.to_owned();
    }
    role.effort = match role.effort.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
    .into();
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
        let mut settings = AppSettings::default();
        settings.validate().unwrap();
        assert_eq!(settings.lookup_delay_ms, 240);
        assert_eq!(settings.word_card_linger_ms, 1500);
        assert_eq!(settings.overlay_offset_x_px, 20);
        assert_eq!(settings.overlay_offset_y_px, 20);
        assert!(settings.ai_translation_enabled);
        assert_eq!(settings.target_language, "简体中文");
        assert_eq!(settings.ai_base_url, "https://api.deepseek.com");
        assert_eq!(settings.ai_connection_name, "API");
        assert_eq!(settings.ai_protocol, "auto");
        assert_eq!(settings.ai_roles.translation.effort, "medium");
        assert_eq!(settings.translation_effort(), "high");
        assert!(settings.auto_translate);
    }

    #[test]
    fn migrates_the_legacy_single_offset_to_the_horizontal_axis() {
        let settings: AppSettings = serde_json::from_str(r#"{"overlayOffsetPx": 0}"#).unwrap();
        assert_eq!(settings.overlay_offset_x_px, 0);
        assert_eq!(settings.overlay_offset_y_px, 20);
    }

    #[test]
    fn migrates_legacy_ai_model_into_all_role_rows() {
        let mut settings = AppSettings {
            ai_model: "legacy-model".into(),
            ..AppSettings::default()
        };
        settings.validate().unwrap();
        assert_eq!(settings.ai_roles.chat.model, "legacy-model");
        assert_eq!(settings.ai_roles.translation.model, "legacy-model");
        assert_eq!(settings.ai_roles.image.model, "legacy-model");
    }

    #[test]
    fn remembers_models_that_reject_reasoning_effort() {
        let mut settings = AppSettings::default();
        assert!(settings.supports_reasoning_effort("compatible-model"));
        assert!(settings.remember_unsupported_reasoning_model("compatible-model"));
        assert!(!settings.supports_reasoning_effort("compatible-model"));
        assert!(!settings.remember_unsupported_reasoning_model("compatible-model"));
    }

    #[test]
    fn clamps_delay_and_rejects_unsafe_font_names() {
        let mut settings = AppSettings {
            lookup_delay_ms: 20,
            word_card_linger_ms: 0,
            overlay_offset_x_px: 999,
            overlay_offset_y_px: 999,
            body_font: "Segoe UI; background:red".into(),
            ..AppSettings::default()
        };
        settings.validate().unwrap();
        assert_eq!(settings.lookup_delay_ms, 100);
        assert_eq!(settings.word_card_linger_ms, 0);
        assert_eq!(settings.overlay_offset_x_px, 240);
        assert_eq!(settings.overlay_offset_y_px, 240);
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
