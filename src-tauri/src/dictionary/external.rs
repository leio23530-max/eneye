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
        let mut seen = std::collections::HashSet::new();
        self.lookup_with_redirects(word, 0, word)
            .into_iter()
            .filter(|entry| {
                let key = entry
                    .html
                    .clone()
                    .unwrap_or_else(|| entry.definitions.join("\n"));
                seen.insert((entry.dictionary_id.clone(), key))
            })
            .collect()
    }

    fn lookup_with_redirects(
        &self,
        word: &str,
        depth: u8,
        requested: &str,
    ) -> Vec<DictionaryEntry> {
        let Some(reader) = &self.reader else {
            return Vec::new();
        };
        match reader {
            Reader::OpenDict(dictionary) => {
                let entries = dictionary.lookup(word).ok().flatten().unwrap_or_default();
                self.resolve_entries(entries, depth, requested)
            }
            Reader::Mdict(dictionary) => {
                let entries = dictionary.lookup(word).ok().flatten().unwrap_or_default();
                self.resolve_entries(entries, depth, requested)
            }
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

    fn resolve_entries(
        &self,
        entries: Vec<DictEntry>,
        depth: u8,
        requested: &str,
    ) -> Vec<DictionaryEntry> {
        const MAX_REDIRECT_DEPTH: u8 = 4;
        let mut resolved = Vec::new();
        for entry in entries {
            if depth < MAX_REDIRECT_DEPTH
                && let Some(target) = redirect_target(&entry.data)
            {
                let mut followed = self.lookup_with_redirects(&target, depth + 1, requested);
                if !followed.is_empty() {
                    for item in &mut followed {
                        item.priority = item.priority.saturating_add(1000);
                        item.dictionary_name =
                            format!("{}（{} 原形）", item.dictionary_name, target);
                    }
                    resolved.extend(followed);
                    continue;
                }
            }
            resolved.push(from_open_dict_entry(&self.metadata, entry, requested));
        }
        resolved
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

fn redirect_target(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let target = text.strip_prefix("@@@LINK=")?.lines().next()?.trim();
    (!target.is_empty() && target.chars().count() <= 200).then(|| target.to_owned())
}

fn phonetics_in(html: &str) -> Vec<String> {
    let mut rest = html;
    let mut values: Vec<String> = Vec::new();
    while let Some(position) = rest.find("class=\"phon\"") {
        let after = &rest[position..];
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let body = &after[tag_end + 1..];
        let Some(close) = body.find("</span>") else {
            break;
        };
        let value = clean_inline_html(&body[..close]);
        if !value.is_empty() && !values.contains(&value) {
            values.push(value);
        }
        rest = &body[close + 7..];
    }
    values
}

fn accent_block<'a>(html: &'a str, class_name: &str) -> Option<&'a str> {
    let marker = format!("class=\"{class_name}\"");
    let marker_position = html.find(&marker)?;
    let body_start = html[marker_position..].find('>')? + marker_position + 1;
    let body = &html[body_start..];
    let body_end = body.find("</div>")?;
    Some(&html[body_start..body_start + body_end])
}

fn extract_phonetic(html: &str) -> Option<String> {
    let british = accent_block(html, "phons_br")
        .map(phonetics_in)
        .unwrap_or_default();
    let american = accent_block(html, "phons_n_am")
        .map(phonetics_in)
        .unwrap_or_default();
    let mut lines = Vec::new();
    if !british.is_empty() {
        lines.push(format!("英音 {}", british.join("  ")));
    }
    if !american.is_empty() {
        lines.push(format!("美音 {}", american.join("  ")));
    }
    if !lines.is_empty() {
        return Some(lines.join("\n"));
    }
    let values = phonetics_in(html);
    (!values.is_empty()).then(|| values.join("\n"))
}

fn clean_inline_html(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            _ => text.push(character),
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_headword(html: &str) -> Option<String> {
    let marker = "class=\"headword\"";
    let position = html.find(marker)?;
    let after = &html[position..];
    let tag_end = after.find('>')?;
    let body = &after[tag_end + 1..];
    let close = body.find("</h1>")?;
    let headword = clean_inline_html(&body[..close]);
    (!headword.is_empty()).then_some(headword)
}

fn select_entry_html(html: &str, requested: &str) -> Option<String> {
    let target = requested.trim().to_lowercase();
    if target.is_empty() {
        return None;
    }
    let marker = "<div class=\"entry\"";
    let mut starts = Vec::new();
    let mut offset = 0;
    while let Some(position) = html[offset..].find(marker) {
        let absolute = offset + position;
        starts.push(absolute);
        offset = absolute + marker.len();
    }
    let mut selected = String::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(html.len());
        let segment = &html[*start..end];
        if extract_headword(segment).is_some_and(|headword| headword.to_lowercase() == target) {
            selected.push_str(segment);
        }
    }
    (!selected.is_empty()).then_some(selected)
}

fn extract_form_phonetic(html: &str, requested: &str) -> Option<String> {
    let target = requested.trim().to_lowercase();
    if target.is_empty() {
        return None;
    }
    let mut rest = html;
    while let Some(position) = rest.find("<tr") {
        let after = &rest[position..];
        let Some(end) = after.find("</tr>") else {
            break;
        };
        let row = &after[..end + 5];
        let words = html_to_lines(row).join(" ").to_lowercase();
        let matches = words
            .split(|character: char| {
                !character.is_alphabetic() && character != '\'' && character != '-'
            })
            .any(|word| word == target);
        if matches && let Some(phonetic) = extract_phonetic(row) {
            return Some(phonetic);
        }
        rest = &after[end + 5..];
    }
    None
}

fn from_open_dict_entry(
    metadata: &ExternalDictionary,
    entry: DictEntry,
    requested: &str,
) -> DictionaryEntry {
    let mut raw = String::from_utf8_lossy(&entry.data).into_owned();
    let is_html = matches!(entry.type_id, 'h' | 'x') || raw.contains('<');
    if is_html && let Some(selected) = select_entry_html(&raw, requested) {
        raw = selected;
    }
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
        phonetic: if is_html {
            extract_form_phonetic(&raw, requested).or_else(|| extract_phonetic(&raw))
        } else {
            None
        },
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
    fn extracts_phonetics_from_html_entries() {
        let html = r#"<span class="phon">/meɪk/</span><span class="phon">/meɪk/</span><span class="phon">/sɑːlvd/</span>"#;
        assert_eq!(extract_phonetic(html).as_deref(), Some("/meɪk/\n/sɑːlvd/"));
        assert_eq!(extract_phonetic("<span>no phonetic</span>"), None);
    }

    #[test]
    fn extracts_british_and_american_phonetics() {
        let html = r#"<span class="phonetics"><div class="phons_br"><span class="phon">/ˈhelpə(r)/</span></div><div class="phons_n_am"><span class="phon">/ˈhelpər/</span></div></span>"#;
        assert_eq!(
            extract_phonetic(html).as_deref(),
            Some("英音 /ˈhelpə(r)/\n美音 /ˈhelpər/")
        );
    }

    #[test]
    fn selects_the_requested_headword_from_a_bundled_page() {
        let html = r#"<html><body><div class="entry" id="would_1"><h1 class="headword">would</h1><span class="phon">/wʊd/</span></div><div class="entry" id="will_1"><h1 class="headword">will</h1><span class="phon">/wɪl/</span></div></body></html>"#;
        let selected = select_entry_html(html, "would").unwrap();
        assert!(selected.contains("would_1"));
        assert!(!selected.contains("will_1"));
        assert_eq!(extract_phonetic(&selected).as_deref(), Some("/wʊd/"));
    }

    #[test]
    fn extracts_the_inflected_form_phonetic() {
        let html = r#"<table><tr class="verb_form" form="past"><td><span class="vf_prefix">past simple</span> went</td><td><span class="phon">/went/</span></td></tr></table>"#;
        assert_eq!(
            extract_form_phonetic(html, "went").as_deref(),
            Some("/went/")
        );
        assert_eq!(extract_form_phonetic(html, "go"), None);
    }

    #[test]
    fn recognizes_mdx_redirect_entries() {
        assert_eq!(redirect_target(b"@@@LINK=sense\r\n"), Some("sense".into()));
        assert_eq!(
            redirect_target("\u{feff}@@@LINK=run".as_bytes()),
            Some("run".into())
        );
        assert_eq!(redirect_target(b"<html><body>entry</body></html>"), None);
        assert_eq!(redirect_target(b"@@@LINK=   \n"), None);
    }

    #[test]
    fn strips_dictionary_markup_without_leaving_tags() {
        let lines = html_to_lines("<div>first<br>second &amp; third</div>");
        assert!(lines.iter().all(|line| !line.contains('<')));
        assert!(lines.join(" ").contains("first"));
    }
}
