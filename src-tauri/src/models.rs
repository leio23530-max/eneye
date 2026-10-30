use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub dictionary_id: String,
    pub dictionary_name: String,
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_of_speech: Option<String>,
    pub definitions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflections: Option<String>,
    pub examples: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupResult {
    pub word: String,
    pub normalized_word: String,
    pub source: String,
    pub entries: Vec<DictionaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub translation: String,
    pub source: String,
    pub dictionaries: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub settings: crate::settings::AppSettings,
    pub fonts: Vec<String>,
    pub dictionary_status: crate::dictionary::DictionaryStatus,
    pub external_dictionaries: Vec<crate::dictionary::ExternalDictionary>,
    pub ai_key_configured: bool,
    pub ocr_available: bool,
}
