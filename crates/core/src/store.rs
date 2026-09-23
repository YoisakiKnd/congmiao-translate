use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryItem {
    pub id: i64,
    pub source: String,
    pub target: String,
    pub text: String,
    pub created_at: i64,
    pub results: Vec<HistoryResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryResult {
    pub engine: String,
    pub translated: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VocabItem {
    pub id: i64,
    pub word: String,
    pub translation: String,
    pub phonetic: String,
    pub note: String,
    pub created_at: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| Error::Io(err.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|err| Error::Io(err.to_string()))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS cache (
                key TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                target TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS history_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                history_id INTEGER NOT NULL,
                engine TEXT NOT NULL,
                translated TEXT NOT NULL,
                error TEXT
            );
            CREATE TABLE IF NOT EXISTS vocabulary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL,
                translation TEXT NOT NULL,
                phonetic TEXT NOT NULL DEFAULT '',
                note TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                engine TEXT NOT NULL,
                source TEXT NOT NULL,
                target TEXT NOT NULL,
                mode TEXT NOT NULL,
                input_path TEXT NOT NULL,
                output_path TEXT NOT NULL DEFAULT '',
                total INTEGER NOT NULL,
                done_count INTEGER NOT NULL,
                failed_count INTEGER NOT NULL,
                skipped_count INTEGER NOT NULL,
                error TEXT NOT NULL DEFAULT '',
                context TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS job_segments (
                job_id TEXT NOT NULL,
                idx INTEGER NOT NULL,
                original TEXT NOT NULL,
                translated TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL,
                error TEXT NOT NULL DEFAULT '',
                meta TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (job_id, idx)
            );
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn cache_get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row("SELECT text FROM cache WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok()
    }

    pub fn cache_put(&self, key: &str, text: &str) {
        let Ok(conn) = self.conn.lock() else {
            return;
        };
        let _ = conn.execute(
            "INSERT INTO cache (key, text, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET text = excluded.text, created_at = excluded.created_at",
            params![key, text, now()],
        );
    }

    pub fn cache_clear(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute("DELETE FROM cache", [])
            .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }

    pub fn add_history(&self, source: &str, target: &str, text: &str, results: &[HistoryResult]) {
        let Ok(conn) = self.conn.lock() else {
            return;
        };
        let id = conn
            .query_row(
                "INSERT INTO history (source, target, text, created_at) VALUES (?1, ?2, ?3, ?4) RETURNING id",
                params![source, target, text, now()],
                |row| row.get::<_, i64>(0),
            )
            .ok();
        let Some(id) = id else {
            return;
        };
        for result in results {
            let _ = conn.execute(
                "INSERT INTO history_results (history_id, engine, translated, error) VALUES (?1, ?2, ?3, ?4)",
                params![id, result.engine, result.translated, result.error],
            );
        }
    }

    pub fn list_history(&self, query: &str, limit: i64) -> Result<Vec<HistoryItem>> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        let mut statement = conn
            .prepare(
                "SELECT id, source, target, text, created_at FROM history
                 WHERE text LIKE ?1 OR id IN (
                    SELECT history_id FROM history_results WHERE translated LIKE ?1
                 )
                 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        let pattern = format!("%{}%", query.replace('%', ""));
        let rows = statement
            .query_map(params![pattern, limit], |row| {
                Ok(HistoryItem {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    target: row.get(2)?,
                    text: row.get(3)?,
                    created_at: row.get(4)?,
                    results: Vec::new(),
                })
            })
            .map_err(|err| Error::Io(err.to_string()))?;
        let mut items = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Io(err.to_string()))?;
        drop(statement);
        for item in &mut items {
            let mut results = conn
                .prepare(
                    "SELECT engine, translated, error FROM history_results WHERE history_id = ?1",
                )
                .map_err(|err| Error::Io(err.to_string()))?;
            let mapped = results
                .query_map([item.id], |row| {
                    Ok(HistoryResult {
                        engine: row.get(0)?,
                        translated: row.get(1)?,
                        error: row.get(2)?,
                    })
                })
                .map_err(|err| Error::Io(err.to_string()))?;
            for result in mapped {
                item.results
                    .push(result.map_err(|err| Error::Io(err.to_string()))?);
            }
        }
        Ok(items)
    }

    pub fn delete_history(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute("DELETE FROM history_results WHERE history_id = ?1", [id])
            .map_err(|err| Error::Io(err.to_string()))?;
        conn.execute("DELETE FROM history WHERE id = ?1", [id])
            .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute_batch("DELETE FROM history_results; DELETE FROM history;")
            .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }

    pub fn add_word(
        &self,
        word: &str,
        translation: &str,
        phonetic: &str,
        note: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.query_row(
            "INSERT INTO vocabulary (word, translation, phonetic, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
            params![word, translation, phonetic, note, now()],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))
    }

    pub fn list_words(&self) -> Result<Vec<VocabItem>> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        let mut statement = conn
            .prepare(
                "SELECT id, word, translation, phonetic, note, created_at FROM vocabulary ORDER BY id DESC",
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(VocabItem {
                    id: row.get(0)?,
                    word: row.get(1)?,
                    translation: row.get(2)?,
                    phonetic: row.get(3)?,
                    note: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|err| Error::Io(err.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Io(err.to_string()))
    }

    pub fn delete_word(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute("DELETE FROM vocabulary WHERE id = ?1", [id])
            .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }

    pub fn export_csv(&self) -> Result<String> {
        let words = self.list_words()?;
        let mut out = String::from("word,translation,phonetic,note\n");
        for word in words {
            out.push_str(&csv_field(&word.word));
            out.push(',');
            out.push_str(&csv_field(&word.translation));
            out.push(',');
            out.push_str(&csv_field(&word.phonetic));
            out.push(',');
            out.push_str(&csv_field(&word.note));
            out.push('\n');
        }
        Ok(out)
    }

    pub fn export_anki(&self) -> Result<String> {
        let words = self.list_words()?;
        let mut out = String::new();
        for word in words {
            out.push_str(&word.word.replace('\t', " "));
            out.push('\t');
            out.push_str(&word.translation.replace('\t', " "));
            out.push('\n');
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRecord {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub engine: String,
    pub source: String,
    pub target: String,
    pub mode: String,
    pub input_path: String,
    pub output_path: String,
    pub total: i64,
    pub done_count: i64,
    pub failed_count: i64,
    pub skipped_count: i64,
    pub error: String,
    pub context: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSegment {
    pub idx: i64,
    pub original: String,
    pub translated: String,
    pub status: String,
    pub error: String,
    pub meta: String,
}

impl Store {
    pub fn job_insert(&self, job: &JobRecord, segments: &[JobSegment]) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute(
            "INSERT INTO jobs (
                id, kind, status, engine, source, target, mode, input_path, output_path,
                total, done_count, failed_count, skipped_count, error, context, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                job.id, job.kind, job.status, job.engine, job.source, job.target, job.mode,
                job.input_path, job.output_path, job.total, job.done_count, job.failed_count,
                job.skipped_count, job.error, job.context, job.created_at, job.updated_at
            ],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        for segment in segments {
            conn.execute(
                "INSERT INTO job_segments (job_id, idx, original, translated, status, error, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    job.id,
                    segment.idx,
                    segment.original,
                    segment.translated,
                    segment.status,
                    segment.error,
                    segment.meta
                ],
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        }
        Ok(())
    }

    pub fn job_get(&self, id: &str) -> Result<Option<JobRecord>> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        let mut statement = conn
            .prepare(
                "SELECT id, kind, status, engine, source, target, mode, input_path, output_path,
                        total, done_count, failed_count, skipped_count, error, context, created_at, updated_at
                 FROM jobs WHERE id = ?1",
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        let mut rows = statement
            .query_map([id], map_job)
            .map_err(|err| Error::Io(err.to_string()))?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(|err| Error::Io(err.to_string()))?)),
            None => Ok(None),
        }
    }

    pub fn job_segments(&self, id: &str) -> Result<Vec<JobSegment>> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        let mut statement = conn
            .prepare(
                "SELECT idx, original, translated, status, error, meta
                 FROM job_segments WHERE job_id = ?1 ORDER BY idx",
            )
            .map_err(|err| Error::Io(err.to_string()))?;
        let rows = statement
            .query_map([id], |row| {
                Ok(JobSegment {
                    idx: row.get(0)?,
                    original: row.get(1)?,
                    translated: row.get(2)?,
                    status: row.get(3)?,
                    error: row.get(4)?,
                    meta: row.get(5)?,
                })
            })
            .map_err(|err| Error::Io(err.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| Error::Io(err.to_string()))
    }

    pub fn job_set_status(
        &self,
        id: &str,
        status: &str,
        error: &str,
        output_path: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute(
            "UPDATE jobs SET status = ?1, error = ?2, output_path = CASE WHEN ?3 = '' THEN output_path ELSE ?3 END, updated_at = ?4 WHERE id = ?5",
            params![status, error, output_path, now(), id],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }

    pub fn job_finish_segment(
        &self,
        id: &str,
        idx: i64,
        translated: &str,
        status: &str,
        error: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute(
            "UPDATE job_segments SET translated = ?1, status = ?2, error = ?3 WHERE job_id = ?4 AND idx = ?5",
            params![translated, status, error, id, idx],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        drop(conn);
        self.job_recount(id)
    }

    pub fn job_reset_failed(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        conn.execute(
            "UPDATE job_segments SET status = 'pending', error = '' WHERE job_id = ?1 AND status = 'failed'",
            [id],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        drop(conn);
        self.job_recount(id)
    }

    pub fn job_recount(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|err| Error::Io(err.to_string()))?;
        let count = |status: &str| -> Result<i64> {
            conn.query_row(
                "SELECT COUNT(*) FROM job_segments WHERE job_id = ?1 AND status = ?2",
                params![id, status],
                |row| row.get(0),
            )
            .map_err(|err| Error::Io(err.to_string()))
        };
        let done_count = count("done")?;
        let failed_count = count("failed")?;
        let skipped_count = count("skipped")?;
        conn.execute(
            "UPDATE jobs SET done_count = ?1, failed_count = ?2, skipped_count = ?3, updated_at = ?4 WHERE id = ?5",
            params![done_count, failed_count, skipped_count, now(), id],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
        Ok(())
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn map_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
    Ok(JobRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        status: row.get(2)?,
        engine: row.get(3)?,
        source: row.get(4)?,
        target: row.get(5)?,
        mode: row.get(6)?,
        input_path: row.get(7)?,
        output_path: row.get(8)?,
        total: row.get(9)?,
        done_count: row.get(10)?,
        failed_count: row.get(11)?,
        skipped_count: row.get(12)?,
        error: row.get(13)?,
        context: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_history_and_vocabulary_roundtrip() {
        let dir = std::env::temp_dir().join(format!("congmiao-store-{}", std::process::id()));
        let store = Store::open(&dir.join("store.db")).unwrap();
        assert!(store.cache_get("k").is_none());
        store.cache_put("k", "你好");
        assert_eq!(store.cache_get("k").as_deref(), Some("你好"));
        store.cache_clear().unwrap();
        assert!(store.cache_get("k").is_none());

        store.add_history(
            "en",
            "zh",
            "hello",
            &[HistoryResult {
                engine: "echo".into(),
                translated: "hello".into(),
                error: None,
            }],
        );
        let history = store.list_history("hello", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].results[0].engine, "echo");
        store.clear_history().unwrap();
        assert!(store.list_history("", 10).unwrap().is_empty());

        let id = store.add_word("cache", "缓存", "kæʃ", "").unwrap();
        let csv = store.export_csv().unwrap();
        assert!(csv.contains("cache,缓存"));
        let anki = store.export_anki().unwrap();
        assert!(anki.contains("cache\t缓存"));
        store.delete_word(id).unwrap();
        assert!(store.list_words().unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
