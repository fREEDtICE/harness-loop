use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRecord {
    pub workspace_path: PathBuf,
    pub display_name: String,
    pub last_opened_at: DateTime<Utc>,
    pub pinned: bool,
}

/// SQLite-backed storage for workspace history and global key-value settings.
pub struct LoopSmithStore {
    conn: Connection,
}

impl LoopSmithStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create directory {}", parent.display())
            })?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open database {}", db_path.display()))?;

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("failed to set PRAGMA")?;

        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS workspace_history (
                    workspace_path TEXT PRIMARY KEY,
                    display_name   TEXT NOT NULL,
                    last_opened_at TEXT NOT NULL,
                    pinned         INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS global_settings (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );",
            )
            .context("failed to run migrations")?;
        Ok(())
    }

    pub fn upsert_workspace(&self, record: &WorkspaceRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO workspace_history (workspace_path, display_name, last_opened_at, pinned)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(workspace_path) DO UPDATE SET
                     display_name   = excluded.display_name,
                     last_opened_at = excluded.last_opened_at,
                     pinned         = excluded.pinned",
                params![
                    record.workspace_path.display().to_string(),
                    record.display_name,
                    record.last_opened_at.to_rfc3339(),
                    record.pinned as i32,
                ],
            )
            .context("failed to upsert workspace")?;
        Ok(())
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT workspace_path, display_name, last_opened_at, pinned
                 FROM workspace_history
                 ORDER BY pinned DESC, last_opened_at DESC",
            )
            .context("failed to prepare list query")?;

        let rows = stmt
            .query_map([], |row| {
                let path_str: String = row.get(0)?;
                let display_name: String = row.get(1)?;
                let last_opened_str: String = row.get(2)?;
                let pinned: i32 = row.get(3)?;
                Ok((path_str, display_name, last_opened_str, pinned))
            })
            .context("failed to query workspaces")?;

        let mut records = Vec::new();
        for row in rows {
            let (path_str, display_name, ts_str, pinned) =
                row.context("failed to read workspace row")?;
            let last_opened_at = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            records.push(WorkspaceRecord {
                workspace_path: PathBuf::from(path_str),
                display_name,
                last_opened_at,
                pinned: pinned != 0,
            });
        }
        Ok(records)
    }

    pub fn remove_workspace(&self, workspace_path: &Path) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM workspace_history WHERE workspace_path = ?1",
                params![workspace_path.display().to_string()],
            )
            .context("failed to remove workspace")?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM global_settings WHERE key = ?1")
            .context("failed to prepare setting query")?;

        let mut rows = stmt
            .query_map(params![key], |row| row.get::<_, String>(0))
            .context("failed to query setting")?;

        match rows.next() {
            Some(row) => Ok(Some(row.context("failed to read setting")?)),
            None => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO global_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .context("failed to set setting")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn workspace_upsert_and_list_round_trip() {
        let temp = tempdir().expect("tempdir");
        let store = LoopSmithStore::open(temp.path().join("test.db")).expect("open");

        let record = WorkspaceRecord {
            workspace_path: PathBuf::from("/home/user/project-a"),
            display_name: "project-a".to_string(),
            last_opened_at: Utc::now(),
            pinned: false,
        };

        store.upsert_workspace(&record).expect("upsert");
        let listed = store.list_workspaces().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].workspace_path, record.workspace_path);
        assert_eq!(listed[0].display_name, "project-a");
    }

    #[test]
    fn workspace_upsert_updates_existing() {
        let temp = tempdir().expect("tempdir");
        let store = LoopSmithStore::open(temp.path().join("test.db")).expect("open");

        let mut record = WorkspaceRecord {
            workspace_path: PathBuf::from("/home/user/project-a"),
            display_name: "project-a".to_string(),
            last_opened_at: Utc::now(),
            pinned: false,
        };
        store.upsert_workspace(&record).expect("upsert 1");

        record.display_name = "Project A Renamed".to_string();
        record.pinned = true;
        store.upsert_workspace(&record).expect("upsert 2");

        let listed = store.list_workspaces().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].display_name, "Project A Renamed");
        assert!(listed[0].pinned);
    }

    #[test]
    fn workspace_remove() {
        let temp = tempdir().expect("tempdir");
        let store = LoopSmithStore::open(temp.path().join("test.db")).expect("open");

        let record = WorkspaceRecord {
            workspace_path: PathBuf::from("/home/user/project-a"),
            display_name: "project-a".to_string(),
            last_opened_at: Utc::now(),
            pinned: false,
        };
        store.upsert_workspace(&record).expect("upsert");
        store
            .remove_workspace(&record.workspace_path)
            .expect("remove");
        let listed = store.list_workspaces().expect("list");
        assert!(listed.is_empty());
    }

    #[test]
    fn list_workspaces_orders_pinned_first_then_by_date() {
        let temp = tempdir().expect("tempdir");
        let store = LoopSmithStore::open(temp.path().join("test.db")).expect("open");

        let now = Utc::now();
        let older = now - chrono::Duration::hours(1);

        store
            .upsert_workspace(&WorkspaceRecord {
                workspace_path: PathBuf::from("/older-not-pinned"),
                display_name: "older".to_string(),
                last_opened_at: older,
                pinned: false,
            })
            .expect("upsert");

        store
            .upsert_workspace(&WorkspaceRecord {
                workspace_path: PathBuf::from("/newer-not-pinned"),
                display_name: "newer".to_string(),
                last_opened_at: now,
                pinned: false,
            })
            .expect("upsert");

        store
            .upsert_workspace(&WorkspaceRecord {
                workspace_path: PathBuf::from("/older-pinned"),
                display_name: "pinned".to_string(),
                last_opened_at: older,
                pinned: true,
            })
            .expect("upsert");

        let listed = store.list_workspaces().expect("list");
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].display_name, "pinned");
        assert_eq!(listed[1].display_name, "newer");
        assert_eq!(listed[2].display_name, "older");
    }

    #[test]
    fn global_settings_round_trip() {
        let temp = tempdir().expect("tempdir");
        let store = LoopSmithStore::open(temp.path().join("test.db")).expect("open");

        assert_eq!(store.get_setting("theme").expect("get"), None);

        store.set_setting("theme", "dark").expect("set");
        assert_eq!(
            store.get_setting("theme").expect("get"),
            Some("dark".to_string())
        );

        store.set_setting("theme", "light").expect("update");
        assert_eq!(
            store.get_setting("theme").expect("get"),
            Some("light".to_string())
        );
    }
}
