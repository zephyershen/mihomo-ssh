use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ImportedHost, OperationLog, Server};

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
        conn.execute_batch("PRAGMA foreign_keys = ON;")
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
            "#,
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn list_servers(&self) -> Result<Vec<Server>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, alias, display_name, host_name, user, port, identity_file_hint,
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
                    source: row.get(7)?,
                    last_status: row.get(8)?,
                    last_seen_at: row.get(9)?,
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
            SELECT id, alias, display_name, host_name, user, port, identity_file_hint,
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
                    source: row.get(7)?,
                    last_status: row.get(8)?,
                    last_seen_at: row.get(9)?,
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
                  (alias, display_name, host_name, user, port, identity_file_hint,
                   source, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ssh_config', ?7, ?7)
                ON CONFLICT(alias) DO UPDATE SET
                  host_name = excluded.host_name,
                  user = excluded.user,
                  port = excluded.port,
                  identity_file_hint = excluded.identity_file_hint,
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
