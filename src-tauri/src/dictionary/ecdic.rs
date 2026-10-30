use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::models::DictionaryEntry;

use super::{clean_optional_text, ecdict_connection, format_exchange, split_definitions};

pub struct EcdicDictionary {
    connection: Connection,
    entry_count: u64,
}

impl EcdicDictionary {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = ecdict_connection(path)
            .with_context(|| format!("failed to open ECDICT at {}", path.display()))?;
        let entry_count = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'entry_count'",
                [],
                |row| row.get::<_, String>(0),
            )?
            .parse()
            .context("invalid ECDICT entry count")?;
        Ok(Self {
            connection,
            entry_count,
        })
    }

    pub fn entry_count(&self) -> u64 {
        self.entry_count
    }

    pub fn lookup(&self, word: &str, normalized_word: &str) -> Result<Vec<DictionaryEntry>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT word, phonetic, definition, translation, pos, exchange
            FROM entries
            WHERE word_norm = ?1
            ORDER BY CASE WHEN word = ?2 THEN 0 ELSE 1 END,
                     length(word), id
            LIMIT 8
            "#,
        )?;
        let rows = statement.query_map(params![normalized_word, word], |row| {
            let translation: String = row.get(3)?;
            let definition: String = row.get(2)?;
            let phonetic: String = row.get(1)?;
            let pos: String = row.get(4)?;
            let exchange: String = row.get(5)?;
            Ok(DictionaryEntry {
                dictionary_id: "ecdict".into(),
                dictionary_name: "ECDICT".into(),
                priority: 100,
                phonetic: clean_optional_text(phonetic),
                part_of_speech: clean_optional_text(pos),
                definitions: split_definitions(&translation, &definition),
                inflections: format_exchange(&exchange),
                examples: definition
                    .lines()
                    .map(str::trim)
                    .filter(|line| line.starts_with("例"))
                    .take(4)
                    .map(ToOwned::to_owned)
                    .collect(),
                html: None,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_entries_from_a_minimal_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO metadata VALUES ('schema_version', '1');
                INSERT INTO metadata VALUES ('entry_count', '2');
                CREATE TABLE entries (
                    id INTEGER PRIMARY KEY,
                    word TEXT NOT NULL,
                    word_norm TEXT NOT NULL,
                    phonetic TEXT NOT NULL,
                    definition TEXT NOT NULL,
                    translation TEXT NOT NULL,
                    pos TEXT NOT NULL,
                    exchange TEXT NOT NULL
                );
                INSERT INTO entries VALUES
                    (1, 'hello', 'hello', 'həˈləʊ', 'greeting', '你好', 'int.', 'p:helloed');
                "#,
            )
            .unwrap();
        drop(connection);

        let dictionary = EcdicDictionary::open(&path).unwrap();
        let result = dictionary.lookup("Hello", "hello").unwrap();
        assert_eq!(dictionary.entry_count(), 2);
        assert_eq!(result[0].definitions, vec!["你好"]);
        assert_eq!(result[0].part_of_speech.as_deref(), Some("int."));
    }
}
