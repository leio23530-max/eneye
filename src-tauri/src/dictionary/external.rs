use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use opendict::{
    DictEntry, Dictionary as OpenDictionary, mdict::MdictDictionary, stardict::StarDictDictionary,
};
use serde::{Deserialize, Serialize};

use crate::models::DictionaryEntry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalDictionary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub format: String,
    pub priority: i32,
    pub available: bool,
    pub error: Option<String>,
}

enum Reader {
    OpenDict(Box<dyn OpenDictionary + Send + Sync>),
    Mdict(Box<MdictDictionary>),
    Csv(CsvReader),
}

struct CsvReader {
    entries: HashMap<String, Vec<CsvEntry>>,
}

struct CsvEntry {
    phonetic: Option<String>,
    part_of_speech: Option<String>,
    definitions: Vec<String>,
}

pub struct LoadedExternalDictionary {
    pub metadata: ExternalDictionary,
    reader: Option<Reader>,
}

impl LoadedExternalDictionary {
    pub fn open(path: PathBuf) -> Self {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let format = dictionary_format(&canonical).unwrap_or("unknown");
        let fallback_name = canonical
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("未命名词典")
            .to_owned();
        match load_reader(&canonical, format) {
            Ok((reader, name)) => Self {
                metadata: ExternalDictionary {
                    id: uuid::Uuid::new_v4().to_string(),
                    name,
                    path: canonical,
                    format: format.into(),
                    priority: 0,
                    available: true,
                    error: None,
                },
                reader: Some(reader),
            },
            Err(error) => Self {
                metadata: ExternalDictionary {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: fallback_name,
                    path: canonical,
                    format: format.into(),
                    priority: 0,
                    available: false,
                    error: Some(error.to_string()),
                },
                reader: None,
            },
        }
    }

    pub fn from_metadata(metadata: ExternalDictionary) -> Self {
        if !metadata.path.exists() {
            return Self {
                metadata: ExternalDictionary {
                    available: false,
                    error: Some("词典文件不存在，请重新定位".into()),
                    ..metadata
                },
                reader: None,
            };
        }
        let loaded = load_reader(&metadata.path, &metadata.format);
        match loaded {
            Ok((reader, _)) => Self {
                metadata: ExternalDictionary {
                    available: true,
                    error: None,
                    ..metadata
                },
                reader: Some(reader),
            },
            Err(error) => Self {
                metadata: ExternalDictionary {
                    available: false,
                    error: Some(error.to_string()),
                    ..metadata
                },
                reader: None,
            },
        }
    }

    pub fn lookup(&self, word: &str) -> Vec<DictionaryEntry> {
        let Some(reader) = &self.reader else {
            return Vec::new();
        };
        match reader {
            Reader::OpenDict(dictionary) => dictionary
                .lookup(word)
                .ok()
                .flatten()
                .unwrap_or_default()
                .into_iter()
                .map(|entry| from_open_dict_entry(&self.metadata, entry))
                .collect(),
            Reader::Mdict(dictionary) => dictionary
                .lookup(word)
                .ok()
                .flatten()
                .unwrap_or_default()
                .into_iter()
                .map(|entry| from_open_dict_entry(&self.metadata, entry))
                .collect(),
            Reader::Csv(csv) => csv
                .entries
                .get(&normalize(word))
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| DictionaryEntry {
                            dictionary_id: self.metadata.id.clone(),
                            dictionary_name: self.metadata.name.clone(),
                            priority: self.metadata.priority,
                            phonetic: entry.phonetic.clone(),
                            part_of_speech: entry.part_of_speech.clone(),
                            definitions: entry.definitions.clone(),
                            inflections: None,
                            examples: Vec::new(),
                            html: None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn resource(&self, path: &str) -> Option<Vec<u8>> {
        match &self.reader {
            Some(Reader::Mdict(dictionary)) => dictionary.lookup_resource(path),
            _ => None,
        }
    }
}

fn load_reader(path: &Path, format: &str) -> Result<(Reader, String)> {
    match format {
        "csv" => {
            let reader = CsvReader::open(path)?;
            Ok((
                Reader::Csv(reader),
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("CSV 词典")
                    .to_owned(),
            ))
        }
        "mdx" => {
            let directory = path.parent().context("MDX 没有父目录")?;
            let dictionary = MdictDictionary::open(directory)
                .context("无法读取 MDX/MDD，请检查词典是否加密或受损")?;
            let name = dictionary.info().name.clone();
            Ok((Reader::Mdict(Box::new(dictionary)), name))
        }
        "stardict" => {
            let directory = path.parent().context("StarDict 没有父目录")?;
            let dictionary =
                StarDictDictionary::open_dir(directory).context("无法读取 StarDict 词典")?;
            let name = dictionary.info().name.clone();
            Ok((Reader::OpenDict(Box::new(dictionary)), name))
        }
        "open-dict" => {
            let directory = path.parent().context("词典没有父目录")?;
            let dictionary = opendict::open(directory).context("无法识别词典目录")?;
            let name = dictionary.info().name.clone();
            Ok((Reader::OpenDict(dictionary), name))
        }
        _ => anyhow::bail!("不支持的词典格式"),
    }
}

fn dictionary_format(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "csv" => Some("csv"),
        "mdx" => Some("mdx"),
        "ifo" | "idx" | "dict" | "dz" | "syn" => Some("stardict"),
        _ => None,
    }
}

fn from_open_dict_entry(metadata: &ExternalDictionary, entry: DictEntry) -> DictionaryEntry {
    let raw = String::from_utf8_lossy(&entry.data).into_owned();
    let is_html = matches!(entry.type_id, 'h' | 'x') || raw.contains('<');
    let definitions = if is_html {
        html_to_lines(&raw)
    } else {
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .map(ToOwned::to_owned)
            .collect()
    };
    DictionaryEntry {
        dictionary_id: metadata.id.clone(),
        dictionary_name: metadata.name.clone(),
        priority: metadata.priority,
        phonetic: None,
        part_of_speech: None,
        definitions,
        inflections: None,
        examples: Vec::new(),
        html: is_html.then_some(raw),
    }
}

impl CsvReader {
    fn open(path: &Path) -> Result<Self> {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_path(path)
            .context("无法读取 CSV 词典")?;
        let headers = reader.headers()?.clone();
        let column = |candidate: &str| {
            headers
                .iter()
                .position(|header| header.trim().eq_ignore_ascii_case(candidate))
        };
        let translation_index = column("translation").or(column("释义")).unwrap_or(1);
        let phonetic_index = column("phonetic").or(column("音标"));
        let pos_index = column("pos").or(column("词性"));
        let mut entries: HashMap<String, Vec<CsvEntry>> = HashMap::new();
        for row in reader.records() {
            let row = row?;
            let Some(word) = row.get(0).map(str::trim).filter(|word| !word.is_empty()) else {
                continue;
            };
            let Some(translation) = row.get(translation_index) else {
                continue;
            };
            let definitions = translation
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(12)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if definitions.is_empty() {
                continue;
            }
            entries.entry(normalize(word)).or_default().push(CsvEntry {
                phonetic: phonetic_index
                    .and_then(|index| row.get(index))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                part_of_speech: pos_index
                    .and_then(|index| row.get(index))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                definitions,
            });
            if entries.len() > 1_000_000 {
                anyhow::bail!("CSV 词典词条过多，已停止导入");
            }
        }
        Ok(Self { entries })
    }
}

fn normalize(word: &str) -> String {
    word.trim().replace('\u{2019}', "'").to_lowercase()
}

pub fn html_to_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut previous_space = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                if !previous_space {
                    text.push(' ');
                    previous_space = true;
                }
            }
            _ if in_tag => {}
            '&' => text.push(' '),
            character if character.is_whitespace() => {
                if !previous_space {
                    text.push(' ');
                    previous_space = true;
                }
            }
            character => {
                text.push(character);
                previous_space = false;
            }
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    let words = text.split_whitespace().collect::<Vec<_>>();
    words
        .chunks(16)
        .map(|chunk| chunk.join(" "))
        .filter(|line| !line.trim().is_empty())
        .take(12)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn imports_both_ecdict_and_simple_csv_files() {
        let directory = tempfile::tempdir().unwrap();
        let simple = directory.path().join("simple.csv");
        std::fs::File::create(&simple)
            .unwrap()
            .write_all("word,translation\nhello,\u{4f60}\u{597d}\n".as_bytes())
            .unwrap();
        let loaded = LoadedExternalDictionary::open(simple);
        let entries = loaded.lookup("Hello");
        assert_eq!(entries[0].definitions, vec!["你好"]);

        let ecdict = directory.path().join("ecdict.csv");
        std::fs::File::create(&ecdict)
            .unwrap()
            .write_all(
                "word,phonetic,definition,translation,pos\nread,/red/,,\u{9605}\u{8bfb}\u{ff1b}\u{5b66}\u{4e60},v.\n"
                    .as_bytes(),
            )
            .unwrap();
        let loaded = LoadedExternalDictionary::open(ecdict);
        assert_eq!(loaded.lookup("read")[0].phonetic.as_deref(), Some("/red/"));
    }

    #[test]
    fn strips_dictionary_markup_without_leaving_tags() {
        let lines = html_to_lines("<div>first<br>second &amp; third</div>");
        assert!(lines.iter().all(|line| !line.contains('<')));
        assert!(lines.join(" ").contains("first"));
    }
}
