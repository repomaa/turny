use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct CardMapping {
    pub card_id: String,
    pub playlist_uri: String,
    pub playlist_name: Option<String>,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database: {}", path))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS card_mappings (
                card_id TEXT PRIMARY KEY,
                playlist_uri TEXT NOT NULL,
                playlist_name TEXT
            );
            CREATE TABLE IF NOT EXISTS last_card (
                id INTEGER PRIMARY KEY DEFAULT 1,
                card_id TEXT
            );",
        )
        .context("Failed to create database tables")?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn add_card_mapping(&self, card_id: &str, playlist_uri: &str, playlist_name: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO card_mappings (card_id, playlist_uri, playlist_name) VALUES (?1, ?2, ?3)",
            rusqlite::params![card_id, playlist_uri, playlist_name],
        )
        .context("Failed to add card mapping")?;
        Ok(())
    }

    pub fn remove_card_mapping(&self, card_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        conn.execute(
            "DELETE FROM card_mappings WHERE card_id = ?1",
            rusqlite::params![card_id],
        )
        .context("Failed to remove card mapping")?;
        Ok(())
    }

    pub fn get_playlist_for_card(&self, card_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT playlist_uri FROM card_mappings WHERE card_id = ?1")
            .context("Failed to prepare query")?;
        let result = stmt
            .query_row(rusqlite::params![card_id], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(result)
    }

    pub fn get_all_mappings(&self) -> Result<Vec<CardMapping>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT card_id, playlist_uri, playlist_name FROM card_mappings")
            .context("Failed to prepare query")?;
        let mappings = stmt
            .query_map([], |row| {
                Ok(CardMapping {
                    card_id: row.get(0)?,
                    playlist_uri: row.get(1)?,
                    playlist_name: row.get(2)?,
                })
            })
            .context("Failed to query mappings")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect mappings")?;
        Ok(mappings)
    }

    pub fn set_last_card(&self, card_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO last_card (id, card_id) VALUES (1, ?1)",
            rusqlite::params![card_id],
        )
        .context("Failed to set last card")?;
        Ok(())
    }

    pub fn get_last_card(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT card_id FROM last_card WHERE id = 1")
            .context("Failed to prepare query")?;
        let result = stmt
            .query_row([], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(result)
    }

    pub fn migrate_from_config(
        &self,
        playlists: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("DB lock error: {}", e))?;
        for (card_id, playlist_uri) in playlists {
            conn.execute(
                "INSERT OR IGNORE INTO card_mappings (card_id, playlist_uri) VALUES (?1, ?2)",
                rusqlite::params![card_id, playlist_uri],
            )
            .context("Failed to migrate card mapping")?;
        }
        Ok(())
    }
}
