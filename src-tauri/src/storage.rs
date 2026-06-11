use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{
    BackupFile, BackupSnapshot, ImportedHost, ManualServerInput, OperationLog, Server,
    SubscriptionInput, SubscriptionProfile,
};

#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
}

impl Storage {
    pub fn new(app_dir: impl AsRef<Path>) -> Result<Self, String> {
        let app_dir = app_dir.as_ref();
        std::fs::create_dir_all(app_dir).map_err(|err| err.to_string())?;
        let storage = Self {
            db_path: app_dir.join("mihomo-manager.sqlite3"),
        };
        storage.init()?;
        Ok(storage)
    }

    fn connect(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|err| err.to_string())?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )
        .map_err(|err| err.to_string())?;
        Ok(conn)
    }

    fn init(&self) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS servers (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              alias TEXT NOT NULL UNIQUE,
              display_name TEXT NOT NULL,
              host_name TEXT NOT NULL,
              user TEXT,
              port INTEGER,
              identity_file_hint TEXT,
              identity_file_path TEXT,
              source TEXT NOT NULL,
              last_status TEXT,
              last_seen_at TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS operation_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              server_id INTEGER,
              action TEXT NOT NULL,
              status TEXT NOT NULL,
              message TEXT NOT NULL,
              created_at TEXT NOT NULL,
              FOREIGN KEY(server_id) REFERENCES servers(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS subscriptions (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              url TEXT NOT NULL UNIQUE,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              last_used_at TEXT
            );

            CREATE TABLE IF NOT EXISTS backup_snapshots (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              server_id INTEGER NOT NULL,
              reason TEXT NOT NULL,
              label TEXT,
              remote_dir TEXT NOT NULL UNIQUE,
              files_json TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              FOREIGN KEY(server_id) REFERENCES servers(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_operation_logs_server_id_id
              ON operation_logs(server_id, id DESC);
            CREATE INDEX IF NOT EXISTS idx_backup_snapshots_server_id_id
              ON backup_snapshots(server_id, id DESC);
            "#,
        )
        .map_err(|err| err.to_string())?;
        add_column_if_missing(&conn, "servers", "identity_file_path", "TEXT")?;
        Ok(())
    }

    pub fn list_servers(&self) -> Result<Vec<Server>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, alias, display_name, host_name, user, port, identity_file_hint, identity_file_path,
                       source, last_status, last_seen_at
                FROM servers
                ORDER BY display_name COLLATE NOCASE, alias COLLATE NOCASE
                "#,
            )
            .map_err(|err| err.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Server {
                    id: row.get(0)?,
                    alias: row.get(1)?,
                    display_name: row.get(2)?,
                    host_name: row.get(3)?,
                    user: row.get(4)?,
                    port: row.get::<_, Option<i64>>(5)?.map(|p| p as u16),
                    identity_file_hint: row.get(6)?,
                    identity_file_path: row.get(7)?,
                    source: row.get(8)?,
                    last_status: row.get(9)?,
                    last_seen_at: row.get(10)?,
                })
            })
            .map_err(|err| err.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())
    }

    pub fn get_server(&self, id: i64) -> Result<Server, String> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT id, alias, display_name, host_name, user, port, identity_file_hint, identity_file_path,
                   source, last_status, last_seen_at
            FROM servers
            WHERE id = ?1
            "#,
            params![id],
            |row| {
                Ok(Server {
                    id: row.get(0)?,
                    alias: row.get(1)?,
                    display_name: row.get(2)?,
                    host_name: row.get(3)?,
                    user: row.get(4)?,
                    port: row.get::<_, Option<i64>>(5)?.map(|p| p as u16),
                    identity_file_hint: row.get(6)?,
                    identity_file_path: row.get(7)?,
                    source: row.get(8)?,
                    last_status: row.get(9)?,
                    last_seen_at: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("server {id} not found"))
    }

    pub fn upsert_imported_hosts(&self, hosts: &[ImportedHost]) -> Result<Vec<Server>, String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|err| err.to_string())?;
        let now = Utc::now().to_rfc3339();

        for host in hosts {
            tx.execute(
                r#"
                INSERT INTO servers
                  (alias, display_name, host_name, user, port, identity_file_hint, identity_file_path,
                   source, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'ssh_config', ?7, ?7)
                ON CONFLICT(alias) DO UPDATE SET
                  host_name = excluded.host_name,
                  user = excluded.user,
                  port = excluded.port,
                  identity_file_hint = excluded.identity_file_hint,
                  identity_file_path = excluded.identity_file_path,
                  source = excluded.source,
                  updated_at = excluded.updated_at
                "#,
                params![
                    host.alias,
                    host.alias,
                    host.host_name,
                    host.user,
                    host.port.map(i64::from),
                    host.identity_file_hint,
                    now
                ],
            )
            .map_err(|err| err.to_string())?;
        }

        tx.commit().map_err(|err| err.to_string())?;
        self.list_servers()
    }

    pub fn upsert_manual_server(
        &self,
        input: &ManualServerInput,
        identity_file_path: &Path,
        identity_file_hint: &str,
    ) -> Result<Vec<Server>, String> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let port = input.port.unwrap_or(22);
        let alias = manual_alias(&input.user, &input.host_name, port);
        let display_name = input
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&input.host_name);

        conn.execute(
            r#"
            INSERT INTO servers
              (alias, display_name, host_name, user, port, identity_file_hint, identity_file_path,
               source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'manual', ?8, ?8)
            ON CONFLICT(alias) DO UPDATE SET
              display_name = excluded.display_name,
              host_name = excluded.host_name,
              user = excluded.user,
              port = excluded.port,
              identity_file_hint = excluded.identity_file_hint,
              identity_file_path = excluded.identity_file_path,
              source = excluded.source,
              updated_at = excluded.updated_at
            "#,
            params![
                alias,
                display_name,
                input.host_name,
                input.user,
                i64::from(port),
                identity_file_hint,
                identity_file_path.to_string_lossy().to_string(),
                now
            ],
        )
        .map_err(|err| err.to_string())?;
        self.list_servers()
    }

    pub fn list_subscriptions(&self) -> Result<Vec<SubscriptionProfile>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, name, url, created_at, updated_at, last_used_at
                FROM subscriptions
                ORDER BY COALESCE(last_used_at, updated_at) DESC, name COLLATE NOCASE
                "#,
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], subscription_from_row)
            .map_err(|err| err.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())
    }

    pub fn get_subscription(&self, subscription_id: i64) -> Result<SubscriptionProfile, String> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT id, name, url, created_at, updated_at, last_used_at
            FROM subscriptions
            WHERE id = ?1
            "#,
            params![subscription_id],
            subscription_from_row,
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("subscription {subscription_id} not found"))
    }

    pub fn save_subscription(
        &self,
        input: &SubscriptionInput,
    ) -> Result<SubscriptionProfile, String> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Subscription");

        if let Some(id) = input.id {
            let updated = conn
                .execute(
                    r#"
                    UPDATE subscriptions
                    SET name = ?1, url = ?2, updated_at = ?3
                    WHERE id = ?4
                    "#,
                    params![name, input.url.as_str(), now, id],
                )
                .map_err(|err| err.to_string())?;
            if updated == 0 {
                return Err(format!("subscription {id} not found"));
            }
            return self.get_subscription(id);
        }

        conn.execute(
            r#"
            INSERT INTO subscriptions (name, url, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?3)
            ON CONFLICT(url) DO UPDATE SET
              name = excluded.name,
              updated_at = excluded.updated_at
            "#,
            params![name, input.url.as_str(), now],
        )
        .map_err(|err| err.to_string())?;
        self.get_subscription_by_url(&input.url)
    }

    pub fn delete_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionProfile>, String> {
        let conn = self.connect()?;
        let deleted = conn
            .execute(
                "DELETE FROM subscriptions WHERE id = ?1",
                params![subscription_id],
            )
            .map_err(|err| err.to_string())?;
        if deleted == 0 {
            return Err(format!("subscription {subscription_id} not found"));
        }
        self.list_subscriptions()
    }

    pub fn mark_subscription_used(
        &self,
        subscription_id: i64,
    ) -> Result<SubscriptionProfile, String> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let updated = conn
            .execute(
                "UPDATE subscriptions SET last_used_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, subscription_id],
            )
            .map_err(|err| err.to_string())?;
        if updated == 0 {
            return Err(format!("subscription {subscription_id} not found"));
        }
        self.get_subscription(subscription_id)
    }

    pub fn insert_backup_snapshot(
        &self,
        server_id: i64,
        reason: &str,
        label: Option<&str>,
        remote_dir: &str,
        files: &[BackupFile],
    ) -> Result<BackupSnapshot, String> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let files_json = serde_json::to_string(files).map_err(|err| err.to_string())?;
        conn.execute(
            r#"
            INSERT INTO backup_snapshots
              (server_id, reason, label, remote_dir, files_json, status, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'ok', ?6)
            "#,
            params![server_id, reason, label, remote_dir, files_json, now],
        )
        .map_err(|err| err.to_string())?;
        let id = conn.last_insert_rowid();
        self.get_backup_snapshot(server_id, id)
    }

    pub fn list_backup_snapshots(&self, server_id: i64) -> Result<Vec<BackupSnapshot>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, server_id, reason, label, remote_dir, files_json, status, created_at
                FROM backup_snapshots
                WHERE server_id = ?1
                ORDER BY id DESC
                "#,
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map(params![server_id], backup_from_row)
            .map_err(|err| err.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())
    }

    pub fn get_backup_snapshot(
        &self,
        server_id: i64,
        backup_id: i64,
    ) -> Result<BackupSnapshot, String> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT id, server_id, reason, label, remote_dir, files_json, status, created_at
            FROM backup_snapshots
            WHERE server_id = ?1 AND id = ?2
            "#,
            params![server_id, backup_id],
            backup_from_row,
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("backup {backup_id} not found"))
    }

    pub fn update_backup_status(&self, backup_id: i64, status: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE backup_snapshots SET status = ?1 WHERE id = ?2",
            params![status, backup_id],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn delete_backup_record(&self, backup_id: i64) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM backup_snapshots WHERE id = ?1",
            params![backup_id],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn get_subscription_by_url(&self, url: &str) -> Result<SubscriptionProfile, String> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT id, name, url, created_at, updated_at, last_used_at
            FROM subscriptions
            WHERE url = ?1
            "#,
            params![url],
            subscription_from_row,
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "subscription not found after save".to_string())
    }

    pub fn update_status(&self, server_id: i64, status: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE servers SET last_status = ?1, last_seen_at = ?2, updated_at = ?2 WHERE id = ?3",
            params![status, Utc::now().to_rfc3339(), server_id],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn delete_server(&self, server_id: i64) -> Result<Vec<Server>, String> {
        let conn = self.connect()?;
        let deleted = conn
            .execute("DELETE FROM servers WHERE id = ?1", params![server_id])
            .map_err(|err| err.to_string())?;
        if deleted == 0 {
            return Err(format!("server {server_id} not found"));
        }
        self.list_servers()
    }

    pub fn add_log(
        &self,
        server_id: Option<i64>,
        action: &str,
        status: &str,
        message: &str,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO operation_logs (server_id, action, status, message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![server_id, action, status, message, Utc::now().to_rfc3339()],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn list_logs(
        &self,
        server_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<OperationLog>, String> {
        let conn = self.connect()?;
        let limit = i64::from(limit.clamp(1, 500));

        if let Some(server_id) = server_id {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, server_id, action, status, message, created_at
                    FROM operation_logs
                    WHERE server_id = ?1
                    ORDER BY id DESC
                    LIMIT ?2
                    "#,
                )
                .map_err(|err| err.to_string())?;
            let rows = stmt
                .query_map(params![server_id, limit], log_from_row)
                .map_err(|err| err.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|err| err.to_string())
        } else {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, server_id, action, status, message, created_at
                    FROM operation_logs
                    ORDER BY id DESC
                    LIMIT ?1
                    "#,
                )
                .map_err(|err| err.to_string())?;
            let rows = stmt
                .query_map(params![limit], log_from_row)
                .map_err(|err| err.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|err| err.to_string())
        }
    }
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| err.to_string())?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
            [],
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn manual_alias(user: &str, host_name: &str, port: u16) -> String {
    format!("manual:{}@{}:{}", user.trim(), host_name.trim(), port)
}

fn log_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationLog> {
    Ok(OperationLog {
        id: row.get(0)?,
        server_id: row.get(1)?,
        action: row.get(2)?,
        status: row.get(3)?,
        message: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn backup_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackupSnapshot> {
    let files_json: String = row.get(5)?;
    let files = serde_json::from_str::<Vec<BackupFile>>(&files_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(BackupSnapshot {
        id: row.get(0)?,
        server_id: row.get(1)?,
        reason: row.get(2)?,
        label: row.get(3)?,
        remote_dir: row.get(4)?,
        files,
        status: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn subscription_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubscriptionProfile> {
    Ok(SubscriptionProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        last_used_at: row.get(5)?,
    })
}
