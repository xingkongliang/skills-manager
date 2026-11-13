//! Rebuildable local cache. A broken cache must never stop the skill database.
use std::collections::HashMap;
use std::sync::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const BUDGET: i64 = 50 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
pub struct CacheEntry { pub cache_key: String, pub text: String, pub target_language: String }
#[derive(Serialize)]
pub struct CacheResult { pub epoch: u64, pub entries: Vec<CacheEntry>, pub persistent: bool }
struct CacheState {
    db: Option<Connection>,
    initialized: bool,
    memory: HashMap<String, (CacheEntry, i64)>,
    epoch: u64,
}
pub struct TranslationCache { inner: Mutex<CacheState> }
impl Default for TranslationCache {
    fn default() -> Self { Self { inner: Mutex::new(CacheState { db: None, initialized: false, memory: HashMap::new(), epoch: 0 }) } }
}

fn open_cache() -> rusqlite::Result<Connection> {
    let dir = super::central_repo::cache_dir();
    std::fs::create_dir_all(&dir).map_err(|_| rusqlite::Error::InvalidPath(dir.clone()))?;
    let db = Connection::open(dir.join("translations.sqlite3"))?;
    db.busy_timeout(std::time::Duration::from_millis(500))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > 1 { return Err(rusqlite::Error::InvalidQuery); }
    db.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;
        CREATE TABLE IF NOT EXISTS translation_cache (
            cache_key TEXT PRIMARY KEY, translated_text TEXT NOT NULL,
            target_language TEXT NOT NULL, last_accessed_at INTEGER NOT NULL, byte_size INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS translation_cache_lru ON translation_cache(last_accessed_at);
        PRAGMA user_version=1;")?;
    Ok(db)
}

impl CacheState {
    fn initialize(&mut self) {
        if !self.initialized { self.db = open_cache().ok(); self.initialized = true; }
    }
    fn read(&self, keys: &[String], now: i64) -> rusqlite::Result<Vec<CacheEntry>> {
        let Some(db) = &self.db else { return Ok(Vec::new()); };
        let mut entries = Vec::new();
        let mut query = db.prepare("SELECT translated_text, target_language FROM translation_cache WHERE cache_key=?1")?;
        for key in keys {
            let mut rows = query.query([key])?;
            if let Some(row) = rows.next()? {
                entries.push(CacheEntry { cache_key: key.clone(), text: row.get(0)?, target_language: row.get(1)? });
                db.execute("UPDATE translation_cache SET last_accessed_at=?1 WHERE cache_key=?2", params![now, key])?;
            }
        }
        Ok(entries)
    }
    fn write(&mut self, entries: &[CacheEntry], now: i64) -> rusqlite::Result<()> {
        let Some(db) = &mut self.db else { return Ok(()); };
        let tx = db.transaction()?;
        for e in entries {
            tx.execute("INSERT OR REPLACE INTO translation_cache VALUES (?1, ?2, ?3, ?4, ?5)", params![e.cache_key, e.text, e.target_language, now, (e.text.len() + e.cache_key.len()) as i64])?;
        }
        let mut size: i64 = tx.query_row("SELECT COALESCE(SUM(byte_size),0) FROM translation_cache", [], |r| r.get(0))?;
        while size > BUDGET {
            let (key, bytes): (String, i64) = tx.query_row("SELECT cache_key, byte_size FROM translation_cache ORDER BY last_accessed_at LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            tx.execute("DELETE FROM translation_cache WHERE cache_key=?1", [key])?;
            size -= bytes;
        }
        tx.commit()?;
        Ok(())
    }
}

impl TranslationCache {
    pub fn get(&self, keys: &[String]) -> CacheResult {
        let mut state = self.inner.lock().unwrap();
        state.initialize();
        let now = chrono::Utc::now().timestamp_millis();
        let mut entries = match state.read(keys, now) { Ok(entries) => entries, Err(_) => { state.db = None; Vec::new() } };
        for key in keys {
            if !entries.iter().any(|e| &e.cache_key == key) {
                if let Some((e, at)) = state.memory.get_mut(key) { *at = now; entries.push(e.clone()); }
            }
        }
        CacheResult { epoch: state.epoch, entries, persistent: state.db.is_some() }
    }
    pub fn put(&self, entries: Vec<CacheEntry>, epoch: u64) -> bool {
        let mut state = self.inner.lock().unwrap();
        if epoch != state.epoch { return false; }
        state.initialize();
        let now = chrono::Utc::now().timestamp_millis();
        if state.write(&entries, now).is_err() { state.db = None; }
        if state.db.is_none() {
            for e in entries { state.memory.insert(e.cache_key.clone(), (e, now)); }
            let mut bytes: usize = state.memory.values().map(|(e, _)| e.text.len() + e.cache_key.len()).sum();
            while bytes > BUDGET as usize {
                let Some(key) = state.memory.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone()) else { break; };
                if let Some((e, _)) = state.memory.remove(&key) { bytes -= e.text.len() + e.cache_key.len(); }
            }
        }
        true
    }
    pub fn clear(&self) -> u64 {
        let mut state = self.inner.lock().unwrap();
        state.epoch += 1;
        state.memory.clear();
        state.initialize();
        if let Some(db) = &state.db {
            if db.execute("DELETE FROM translation_cache", []).is_err() {
                state.db = None;
                // Retire a corrupt cache instead of returning old rows next launch.
                let _ = std::fs::remove_file(super::central_repo::cache_dir().join("translations.sqlite3"));
            } else { let _ = db.execute_batch("PRAGMA incremental_vacuum;"); }
        }
        state.epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory_cache() -> TranslationCache {
        TranslationCache { inner: Mutex::new(CacheState { db: None, initialized: true, memory: HashMap::new(), epoch: 0 }) }
    }
    #[test]
    fn clear_rejects_late_results() {
        let cache = memory_cache();
        let entry = CacheEntry { cache_key: "key".into(), text: "译文".into(), target_language: "zh-CN".into() };
        assert!(cache.put(vec![entry.clone()], 0));
        assert_eq!(cache.get(&["key".into()]).entries.len(), 1);
        cache.clear();
        assert!(!cache.put(vec![entry], 0));
        assert!(cache.get(&["key".into()]).entries.is_empty());
    }
    #[test]
    fn isolated_database_retains_entries_after_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("translations.sqlite3");
        let db = Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE translation_cache(cache_key TEXT PRIMARY KEY,translated_text TEXT,target_language TEXT,last_accessed_at INTEGER,byte_size INTEGER)").unwrap();
        let mut state = CacheState { db: Some(db), initialized: true, memory: HashMap::new(), epoch: 0 };
        let entry = CacheEntry { cache_key: "key".into(), text: "hello".into(), target_language: "zh-CN".into() };
        state.write(&[entry], 1).unwrap();
        assert_eq!(state.read(&["key".into()], 2).unwrap()[0].text, "hello");
        drop(state);

        let mut state = CacheState { db: Some(Connection::open(database).unwrap()), initialized: true, memory: HashMap::new(), epoch: 0 };
        assert_eq!(state.read(&["key".into()], 3).unwrap()[0].text, "hello");
        let replacement = CacheEntry { cache_key: "key".into(), text: "updated".into(), target_language: "zh-CN".into() };
        state.write(&[replacement], 4).unwrap();
        assert_eq!(state.read(&["key".into()], 5).unwrap()[0].text, "updated");
    }
}
