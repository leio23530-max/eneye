use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::dictionary::normalize_word;
use crate::models::{HistoryRecord, LookupResult, Notebook, NotebookWord};

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
            PRAGMA foreign_keys = ON;
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
            CREATE TABLE IF NOT EXISTS notebooks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS notebook_words (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                notebook_id INTEGER NOT NULL REFERENCES notebooks(id) ON DELETE CASCADE,
                word TEXT NOT NULL,
                normalized TEXT NOT NULL,
                phonetic TEXT NOT NULL DEFAULT '',
                definition TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                UNIQUE(notebook_id, normalized)
            );
            CREATE INDEX IF NOT EXISTS notebook_words_notebook
                ON notebook_words(notebook_id, id DESC);
            "#,
        )?;
        ensure_default_notebooks(&connection)?;
        Ok(Self { connection })
    }

    pub fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT n.id, n.name, n.created_at, COUNT(w.id)
            FROM notebooks n
            LEFT JOIN notebook_words w ON w.notebook_id = n.id
            GROUP BY n.id
            ORDER BY n.id ASC
            "#,
        )?;
        let rows = statement.query_map([], notebook_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_notebook(&self, name: &str) -> Result<Notebook> {
        let name = clean_notebook_name(name)?;
        self.connection
            .execute(
                "INSERT INTO notebooks (name, created_at) VALUES (?1, ?2)",
                params![name, now()],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    anyhow::anyhow!("已有同名的本子")
                } else {
                    error.into()
                }
            })?;
        self.notebook(self.connection.last_insert_rowid())
    }

    pub fn delete_notebook(&self, id: i64) -> Result<()> {
        self.connection
            .execute("DELETE FROM notebooks WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_notebook_words(
        &self,
        notebook_id: i64,
        query: Option<&str>,
        limit: u32,
    ) -> Result<Vec<NotebookWord>> {
        let limit = limit.clamp(1, 500) as i64;
        let query = query.unwrap_or_default().trim();
        let mut statement = if query.is_empty() {
            self.connection.prepare(
                r#"
                SELECT id, notebook_id, word, phonetic, definition, created_at
                FROM notebook_words
                WHERE notebook_id = ?1
                ORDER BY id DESC LIMIT ?2
                "#,
            )?
        } else {
            self.connection.prepare(
                r#"
                SELECT id, notebook_id, word, phonetic, definition, created_at
                FROM notebook_words
                WHERE notebook_id = ?1
                  AND (word LIKE '%' || ?2 || '%' OR definition LIKE '%' || ?2 || '%')
                ORDER BY id DESC LIMIT ?3
                "#,
            )?
        };
        let rows: Vec<rusqlite::Result<NotebookWord>> = if query.is_empty() {
            statement
                .query_map(params![notebook_id, limit], notebook_word_from_row)?
                .collect()
        } else {
            statement
                .query_map(params![notebook_id, query, limit], notebook_word_from_row)?
                .collect()
        };
        Ok(rows.into_iter().collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_word_to_notebook(
        &self,
        notebook_id: i64,
        word: &str,
        phonetic: &str,
        definition: &str,
    ) -> Result<NotebookWord> {
        let word = word.trim();
        let normalized = normalize_word(word);
        if word.is_empty() || normalized.is_empty() || word.chars().count() > 96 {
            anyhow::bail!("不是可收藏的单词");
        }
        let phonetic = phonetic.trim().chars().take(120).collect::<String>();
        let definition = definition.trim().chars().take(2_000).collect::<String>();
        self.connection.execute(
            r#"
            INSERT INTO notebook_words (
                notebook_id, word, normalized, phonetic, definition, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(notebook_id, normalized) DO UPDATE SET
                word = excluded.word,
                phonetic = excluded.phonetic,
                definition = excluded.definition
            "#,
            params![notebook_id, word, normalized, phonetic, definition, now()],
        )?;
        self.connection
            .query_row(
                r#"
                SELECT id, notebook_id, word, phonetic, definition, created_at
                FROM notebook_words
                WHERE notebook_id = ?1 AND normalized = ?2
                "#,
                params![notebook_id, normalized],
                notebook_word_from_row,
            )
            .map_err(Into::into)
    }

    pub fn remove_notebook_word(&self, id: i64) -> Result<()> {
        self.connection
            .execute("DELETE FROM notebook_words WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn notebook(&self, id: i64) -> Result<Notebook> {
        self.connection
            .query_row(
                r#"
                SELECT n.id, n.name, n.created_at, COUNT(w.id)
                FROM notebooks n
                LEFT JOIN notebook_words w ON w.notebook_id = n.id
                WHERE n.id = ?1
                GROUP BY n.id
                "#,
                params![id],
                notebook_from_row,
            )
            .map_err(Into::into)
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

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn clean_notebook_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("本子名称不能为空");
    }
    if name.chars().count() > 24 {
        anyhow::bail!("本子名称最多 24 个字符");
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("本子名称不能包含换行或控制字符");
    }
    Ok(name.to_owned())
}

fn ensure_default_notebooks(connection: &Connection) -> Result<()> {
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM notebooks", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let created_at = now();
    for name in ["生词本", "好词本", "坏词本"] {
        connection.execute(
            "INSERT INTO notebooks (name, created_at) VALUES (?1, ?2)",
            params![name, created_at],
        )?;
    }
    Ok(())
}

fn notebook_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notebook> {
    Ok(Notebook {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        word_count: row.get(3)?,
    })
}

fn notebook_word_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotebookWord> {
    Ok(NotebookWord {
        id: row.get(0)?,
        notebook_id: row.get(1)?,
        word: row.get(2)?,
        phonetic: row.get(3)?,
        definition: row.get(4)?,
        created_at: row.get(5)?,
    })
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

    #[test]
    fn starts_with_default_notebooks() {
        let storage = storage();
        let names = storage
            .list_notebooks()
            .unwrap()
            .into_iter()
            .map(|notebook| notebook.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["生词本", "好词本", "坏词本"]);
    }

    #[test]
    fn creates_notebooks_and_deduplicates_words() {
        let storage = storage();
        let notebook = storage.create_notebook("好句本").unwrap();
        assert_eq!(notebook.word_count, 0);

        let word = storage
            .add_word_to_notebook(notebook.id, "Serendipity", "/ˌserənˈdɪpəti/", "意外之喜")
            .unwrap();
        assert_eq!(word.word, "Serendipity");
        assert_eq!(
            storage
                .list_notebook_words(notebook.id, None, 20)
                .unwrap()
                .len(),
            1
        );

        storage
            .add_word_to_notebook(notebook.id, "serendipity", "", "更新释义")
            .unwrap();
        let words = storage.list_notebook_words(notebook.id, None, 20).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].definition, "更新释义");
        assert_eq!(
            storage
                .list_notebooks()
                .unwrap()
                .into_iter()
                .find(|item| item.id == notebook.id)
                .unwrap()
                .word_count,
            1
        );

        storage.remove_notebook_word(word.id).unwrap();
        assert!(
            storage
                .list_notebook_words(notebook.id, None, 20)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_duplicate_notebook_names() {
        let storage = storage();
        storage.create_notebook("复习本").unwrap();
        assert!(storage.create_notebook("复习本").is_err());
        assert!(storage.create_notebook("   ").is_err());
    }
}
