mod ecdic;
mod external;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::models::LookupResult;

pub use ecdic::EcdicDictionary;
pub use external::{ExternalDictionary, LoadedExternalDictionary};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryStatus {
    pub available: bool,
    pub name: String,
    pub entry_count: u64,
    pub path: String,
    pub error: Option<String>,
}

pub struct DictionaryManager {
    ecdic: Option<EcdicDictionary>,
    status: DictionaryStatus,
    external: Vec<LoadedExternalDictionary>,
    registry_path: PathBuf,
}

impl DictionaryManager {
    pub fn open(path: PathBuf, registry_path: PathBuf) -> Self {
        let external_configs = load_registry(&registry_path).unwrap_or_default();
        let mut external = external_configs
            .into_iter()
            .map(LoadedExternalDictionary::from_metadata)
            .collect::<Vec<_>>();
        external.sort_by_key(|dictionary| dictionary.metadata.priority);
        match EcdicDictionary::open(&path) {
            Ok(dictionary) => {
                let count = dictionary.entry_count();
                Self {
                    ecdic: Some(dictionary),
                    status: DictionaryStatus {
                        available: true,
                        name: "ECDICT".into(),
                        entry_count: count,
                        path: path.display().to_string(),
                        error: None,
                    },
                    external,
                    registry_path,
                }
            }
            Err(error) => Self {
                ecdic: None,
                status: DictionaryStatus {
                    available: false,
                    name: "ECDICT".into(),
                    entry_count: 0,
                    path: path.display().to_string(),
                    error: Some(error.to_string()),
                },
                external,
                registry_path,
            },
        }
    }

    pub fn status(&self) -> DictionaryStatus {
        self.status.clone()
    }

    pub fn lookup(&self, raw_word: &str, source: &str) -> Result<LookupResult> {
        let word = clean_lookup_word(raw_word)?;
        let normalized_word = normalize_word(&word);
        let mut entries = Vec::new();
        for dictionary in &self.external {
            entries.extend(dictionary.lookup(&word));
        }
        if let Some(dictionary) = &self.ecdic {
            entries.extend(dictionary.lookup(&word, &normalized_word)?);
        }
        entries.sort_by_key(|entry| entry.priority);
        entries.truncate(24);
        Ok(LookupResult {
            word,
            normalized_word,
            source: source.to_owned(),
            entries,
        })
    }

    /// Looks up a word or phrase in every external dictionary, plus ECDICT
    /// when the query is a normal English headword. Unlike the hover path this
    /// intentionally accepts CJK queries so reverse dictionaries can be used.
    pub fn search(&self, raw_query: &str) -> Result<LookupResult> {
        let query = clean_search_query(raw_query)?;
        let normalized_word = normalize_word(&query);
        let mut entries = Vec::new();
        for dictionary in &self.external {
            entries.extend(dictionary.lookup(&query));
        }
        if let Ok(word) = clean_lookup_word(&query)
            && let Some(dictionary) = &self.ecdic
        {
            entries.extend(dictionary.lookup(&word, &normalized_word)?);
        }
        entries.sort_by_key(|entry| entry.priority);
        entries.truncate(24);
        Ok(LookupResult {
            word: query,
            normalized_word,
            source: "search".into(),
            entries,
        })
    }

    pub fn list_external(&self) -> Vec<ExternalDictionary> {
        self.external
            .iter()
            .map(|dictionary| dictionary.metadata.clone())
            .collect()
    }

    pub fn add_external(&mut self, path: PathBuf) -> Result<ExternalDictionary> {
        let mut dictionary = LoadedExternalDictionary::open(path);
        if !dictionary.metadata.available {
            anyhow::bail!(
                "{}",
                dictionary
                    .metadata
                    .error
                    .as_deref()
                    .unwrap_or("无法读取词典")
            );
        }
        dictionary.metadata.priority = self.external.len() as i32;
        let metadata = dictionary.metadata.clone();
        self.external.push(dictionary);
        self.persist_external()?;
        Ok(metadata)
    }

    pub fn remove_external(&mut self, id: &str) -> Result<bool> {
        let original_len = self.external.len();
        self.external
            .retain(|dictionary| dictionary.metadata.id != id);
        let removed = original_len != self.external.len();
        if removed {
            self.number_external();
            self.persist_external()?;
        }
        Ok(removed)
    }

    pub fn reorder_external(&mut self, ids: &[String]) -> Result<()> {
        let mut reordered = Vec::with_capacity(self.external.len());
        for id in ids {
            if let Some(index) = self
                .external
                .iter()
                .position(|dictionary| dictionary.metadata.id == *id)
            {
                reordered.push(self.external.remove(index));
            }
        }
        reordered.append(&mut self.external);
        self.external = reordered;
        self.number_external();
        self.persist_external()
    }

    pub fn resource(&self, dictionary_id: &str, path: &str) -> Option<Vec<u8>> {
        if !safe_resource_path(path) {
            return None;
        }
        self.external
            .iter()
            .find(|dictionary| dictionary.metadata.id == dictionary_id)
            .and_then(|dictionary| dictionary.resource(path))
    }

    fn number_external(&mut self) {
        for (index, dictionary) in self.external.iter_mut().enumerate() {
            dictionary.metadata.priority = index as i32;
        }
    }

    fn persist_external(&self) -> Result<()> {
        if let Some(parent) = self.registry_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_vec_pretty(&self.list_external())?;
        let temporary = self.registry_path.with_extension("json.tmp");
        fs::write(&temporary, content)?;
        if self.registry_path.exists() {
            fs::remove_file(&self.registry_path)?;
        }
        fs::rename(temporary, &self.registry_path)?;
        Ok(())
    }
}

pub fn clean_search_query(raw: &str) -> Result<String> {
    let query = raw.trim();
    if query.is_empty() {
        anyhow::bail!("请输入要查询的词");
    }
    if query.chars().count() > 128 {
        anyhow::bail!("查询内容过长");
    }
    if query.chars().any(char::is_control) {
        anyhow::bail!("查询内容不能包含换行或控制字符");
    }
    Ok(query.to_owned())
}

pub fn clean_lookup_word(raw: &str) -> Result<String> {
    if raw
        .chars()
        .any(|character| character.is_ascii_digit() || character == '_')
    {
        anyhow::bail!("not an English headword");
    }
    let trimmed = raw
        .trim()
        .trim_matches(|character: char| {
            !character.is_ascii_alphabetic() && character != '\'' && character != '-'
        })
        .replace('\u{2019}', "'");
    if trimmed.is_empty()
        || trimmed.len() > 96
        || !trimmed
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_alphabetic() || matches!(character, '\'' | '-'))
    {
        anyhow::bail!("not an English headword");
    }
    Ok(trimmed.to_owned())
}

pub fn normalize_word(word: &str) -> String {
    word.trim().replace('\u{2019}', "'").to_lowercase()
}

pub(crate) fn ecdict_connection(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let schema: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| anyhow::anyhow!("ECDICT database metadata is missing"))?;
    if schema != "1" {
        anyhow::bail!("unsupported ECDICT schema version: {schema}");
    }
    Ok(connection)
}

pub(crate) fn sanitize_dictionary_text(value: &str) -> String {
    value
        .replace("<<", "")
        .replace(">>", "")
        .replace('\0', "")
        .trim()
        .to_owned()
}

pub(crate) fn split_definitions(translation: &str, definition: &str) -> Vec<String> {
    let primary = translation
        .lines()
        .map(sanitize_dictionary_text)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if !primary.is_empty() {
        return primary;
    }
    definition
        .lines()
        .map(sanitize_dictionary_text)
        .filter(|line| !line.is_empty())
        .take(4)
        .collect()
}

pub(crate) fn clean_optional_text(value: String) -> Option<String> {
    let clean = sanitize_dictionary_text(&value);
    if clean.is_empty() { None } else { Some(clean) }
}

pub(crate) fn format_exchange(exchange: &str) -> Option<String> {
    let labels = [
        ('p', "过去式"),
        ('d', "过去分词"),
        ('i', "现在分词"),
        ('3', "第三人称单数"),
        ('r', "比较级"),
        ('t', "最高级"),
        ('s', "复数"),
        ('0', "原形"),
        ('1', "变形"),
    ];
    let mut parts = Vec::new();
    for segment in exchange.split('/') {
        let Some((code, value)) = segment.split_once(':') else {
            continue;
        };
        let Some(value) = clean_optional_text(value.to_owned()) else {
            continue;
        };
        let Some((_, label)) = labels.iter().find(|(key, _)| key.to_string() == code) else {
            continue;
        };
        parts.push(format!("{label}: {value}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("；"))
    }
}

fn load_registry(path: &Path) -> Result<Vec<ExternalDictionary>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read(path)?;
    Ok(serde_json::from_slice(&content)?)
}

fn safe_resource_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path.components().all(|component| {
            !matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_only_single_english_words() {
        assert_eq!(
            clean_lookup_word("\u{201c}Serendipity,\u{201d}").unwrap(),
            "Serendipity"
        );
        assert!(clean_lookup_word("two words").is_err());
        assert!(clean_lookup_word("word123").is_err());
        assert!(clean_lookup_word("-").is_err());
    }

    #[test]
    fn normalizes_curly_apostrophes() {
        assert_eq!(normalize_word("Don\u{2019}t"), "don't");
    }

    #[test]
    fn formats_ecdict_exchange_codes() {
        assert_eq!(
            format_exchange("p:went/d:gone/3:goes").as_deref(),
            Some("过去式: went；过去分词: gone；第三人称单数: goes")
        );
    }

    #[test]
    fn splits_translation_lines_before_english_fallback() {
        let result = split_definitions("第一行\n第二行", "english");
        assert_eq!(result, vec!["第一行", "第二行"]);
    }

    #[test]
    fn accepts_cjk_search_queries() {
        assert_eq!(clean_search_query(" 美丽 ").unwrap(), "美丽");
        assert_eq!(clean_search_query("serendipity").unwrap(), "serendipity");
        assert!(clean_search_query("   ").is_err());
        assert!(clean_search_query("two\nlines").is_err());
    }

    #[test]
    fn searches_chinese_keys_in_external_dictionaries() {
        let directory = tempfile::tempdir().unwrap();
        let csv_path = directory.path().join("zh.csv");
        std::fs::write(&csv_path, "word,translation\n美丽,beautiful\n爱,love\n").unwrap();
        let registry_path = directory.path().join("dictionaries.json");
        let metadata = ExternalDictionary {
            id: "zh".into(),
            name: "中文反查".into(),
            path: csv_path,
            format: "csv".into(),
            priority: 0,
            available: true,
            error: None,
        };
        std::fs::write(&registry_path, serde_json::to_vec(&vec![metadata]).unwrap()).unwrap();
        let manager =
            DictionaryManager::open(directory.path().join("missing.sqlite"), registry_path);
        let result = manager.search("美丽").unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].definitions, vec!["beautiful"]);
    }

    #[test]
    fn rejects_resource_path_traversal() {
        assert!(safe_resource_path(r"images\icon.png"));
        assert!(!safe_resource_path(r"..\secret.txt"));
        assert!(!safe_resource_path(r"C:\secret.txt"));
    }
}
