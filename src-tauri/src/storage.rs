use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{HistoryRecord, LookupResult};

const HISTORY_LIMIT: i64 = 10_000;
const DEDUPE_WINDOW_SECONDS: i64 = 60;

pub struct Storage {
    connection: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL CHECK (kind IN ('word', 'sentence')),
                text TEXT NOT NULL,
                normalized TEXT NOT NULL,
                translation TEXT NOT NULL DEFAULT '',
                source TEXT NOT NULL,
                dictionaries TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS history_created_at
                ON history(created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS history_normalized
                ON history(normalized);
            "#,
        )?;
        Ok(Self { connection })
    }

    pub fn record_lookup(&self, lookup: &LookupResult) -> Result<()> {
        if !self.insert_unless_recent(
            "word",
            &lookup.word,
            &lookup.normalized_word,
            "",
            &lookup.source,
            &serde_json::to_string(
                &lookup
                    .entries
                    .iter()
                    .map(|entry| entry.dictionary_name.as_str())
                    .collect::<Vec<_>>(),
            )?,
        )? {
            return Ok(());
        }
        self.trim()?;
        Ok(())
    }

    pub fn record_translation(&self, text: &str, translation: &str, source: &str) -> Result<()> {
        let normalized = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !self.insert_unless_recent("sentence", text, &normalized, translation, source, "[]")? {
            return Ok(());
        }
        self.trim()?;
        Ok(())
    }

    fn insert_unless_recent(
        &self,
        kind: &str,
        text: &str,
        normalized: &str,
        translation: &str,
        source: &str,
        dictionaries: &str,
    ) -> Result<bool> {
        let threshold = (Utc::now() - chrono::Duration::seconds(DEDUPE_WINDOW_SECONDS))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let recent = self
            .connection
            .query_row(
                r#"
                SELECT 1 FROM history
                WHERE kind = ?1 AND normalized = ?2 AND created_at >= ?3
                LIMIT 1
                "#,
                params![kind, normalized, threshold],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if recent {
            return Ok(false);
        }

        self.connection.execute(
            r#"
            INSERT INTO history (
                kind, text, normalized, translation, source, dictionaries, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                kind,
                text,
                normalized,
                translation,
                source,
                dictionaries,
                Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
            ],
        )?;
        Ok(true)
    }

    pub fn list(&self, query: Option<&str>, limit: u32) -> Result<Vec<HistoryRecord>> {
        let limit = limit.clamp(1, 500) as i64;
        let query = query.unwrap_or_default().trim();
        let mut statement = if query.is_empty() {
            self.connection.prepare(
                r#"
                SELECT id, kind, text, translation, source, dictionaries, created_at
                FROM history ORDER BY id DESC LIMIT ?1
                "#,
            )?
        } else {
            self.connection.prepare(
                r#"
                SELECT id, kind, text, translation, source, dictionaries, created_at
                FROM history
                WHERE text LIKE '%' || ?1 || '%'
                   OR translation LIKE '%' || ?1 || '%'
                ORDER BY id DESC LIMIT ?2
                "#,
            )?
        };

        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HistoryRecord> {
            let dictionaries_json: String = row.get(5)?;
            Ok(HistoryRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                text: row.get(2)?,
                translation: row.get(3)?,
                source: row.get(4)?,
                dictionaries: serde_json::from_str(&dictionaries_json).unwrap_or_default(),
                created_at: row.get(6)?,
            })
        };

        let rows: Vec<rusqlite::Result<HistoryRecord>> = if query.is_empty() {
            statement.query_map(params![limit], mapper)?.collect()
        } else {
            statement
                .query_map(params![query, limit], mapper)?
                .collect()
        };
        Ok(rows.into_iter().collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.connection
            .execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.connection.execute("DELETE FROM history", [])?;
        Ok(())
    }

    fn trim(&self) -> Result<()> {
        self.connection.execute(
            r#"
            DELETE FROM history
            WHERE id NOT IN (
                SELECT id FROM history ORDER BY id DESC LIMIT ?1
            )
            "#,
            params![HISTORY_LIMIT],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DictionaryEntry;

    fn storage() -> Storage {
        Storage::from_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn lookup(word: &str) -> LookupResult {
        LookupResult {
            word: word.into(),
            normalized_word: word.to_lowercase(),
            source: "uia".into(),
            entries: vec![DictionaryEntry {
                dictionary_id: "ecdict".into(),
                dictionary_name: "ECDICT".into(),
                priority: 100,
                phonetic: None,
                part_of_speech: None,
                definitions: vec!["测试".into()],
                inflections: None,
                examples: vec![],
                html: None,
            }],
        }
    }

    #[test]
    fn records_and_searches_history() {
        let storage = storage();
        storage.record_lookup(&lookup("Serendipity")).unwrap();
        storage
            .record_translation("A quiet room", "安静的房间", "uia")
            .unwrap();
        let rows = storage.list(Some("安静"), 20).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "sentence");
    }

    #[test]
    fn suppresses_duplicate_lookups_within_window() {
        let storage = storage();
        storage.record_lookup(&lookup("hello")).unwrap();
        storage.record_lookup(&lookup("hello")).unwrap();
        assert_eq!(storage.list(None, 20).unwrap().len(), 1);
    }

    #[test]
    fn can_delete_and_clear_history() {
        let storage = storage();
        storage.record_lookup(&lookup("one")).unwrap();
        storage.record_lookup(&lookup("two")).unwrap();
        let first = storage.list(None, 20).unwrap().remove(0);
        storage.delete(first.id).unwrap();
        assert_eq!(storage.list(None, 20).unwrap().len(), 1);
        storage.clear().unwrap();
        assert!(storage.list(None, 20).unwrap().is_empty());
    }
}
