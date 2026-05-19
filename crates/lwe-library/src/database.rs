//! SQLite database for wallpaper library persistence
//!
//! Stores wallpaper metadata, thumbnails, and user settings.
//! Features:
//! - Full-text search (FTS5)
//! - Tag system with many-to-many relationships
//! - Favorites with toggle functionality
//! - Usage tracking and statistics

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, info};

use lwe_core::{SourceType, WallpaperItem, WallpaperMetadata, WallpaperType};

/// Wallpaper library database
#[allow(clippy::arc_with_non_send_sync)] // Intentional: Connection is used in single-threaded context
pub struct LibraryDatabase {
    conn: Arc<RwLock<Connection>>,
    #[allow(dead_code)] // Reserved for future use (backup, migration)
    db_path: PathBuf,
}

impl LibraryDatabase {
    /// Open or create database at the given path
    #[allow(clippy::arc_with_non_send_sync)] // Intentional: Connection used in single-threaded context with RwLock
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }

        info!("📦 Opening library database: {}", path.display());

        let conn = Connection::open(path).context("Failed to open database")?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("Failed to set database pragmas")?;

        let db = Self {
            conn: Arc::new(RwLock::new(conn)),
            db_path: path.to_owned(),
        };

        db.initialize_schema()?;

        Ok(db)
    }

    /// Get default database path
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("wayvid")
            .join("library.db")
    }

    fn initialize_schema(&self) -> Result<()> {
        let conn = self.conn.write().unwrap();

        conn.execute_batch(r#"
            -- Wallpaper items table
            CREATE TABLE IF NOT EXISTS wallpapers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_path TEXT NOT NULL UNIQUE,
                source_type TEXT NOT NULL,
                wallpaper_type TEXT NOT NULL,
                thumbnail_path TEXT,
                
                -- Metadata
                title TEXT,
                author TEXT,
                description TEXT,
                tags TEXT,
                duration_secs REAL,
                resolution_w INTEGER,
                resolution_h INTEGER,
                file_size INTEGER,
                workshop_id INTEGER,
                
                -- Timestamps
                added_at TEXT NOT NULL,
                last_used TEXT,
                
                -- User data
                favorite INTEGER NOT NULL DEFAULT 0,
                use_count INTEGER NOT NULL DEFAULT 0,
                rating INTEGER DEFAULT 0
            );

            -- Tags table for efficient querying
            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                color TEXT DEFAULT '#666666',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            -- Wallpaper-tag relationship
            CREATE TABLE IF NOT EXISTS wallpaper_tags (
                wallpaper_id TEXT NOT NULL,
                tag_id INTEGER NOT NULL,
                PRIMARY KEY (wallpaper_id, tag_id),
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            -- Library folders
            CREATE TABLE IF NOT EXISTS folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                enabled INTEGER NOT NULL DEFAULT 1,
                scan_recursive INTEGER NOT NULL DEFAULT 1,
                last_scanned_at TEXT,
                wallpaper_count INTEGER NOT NULL DEFAULT 0
            );

            -- Thumbnails cache
            CREATE TABLE IF NOT EXISTS thumbnails (
                wallpaper_id TEXT PRIMARY KEY,
                data BLOB NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                format TEXT NOT NULL DEFAULT 'webp',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE CASCADE
            );

            -- Collections (user-defined groups)
            CREATE TABLE IF NOT EXISTS collections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                cover_wallpaper_id TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (cover_wallpaper_id) REFERENCES wallpapers(id) ON DELETE SET NULL
            );

            -- Collection-wallpaper relationship
            CREATE TABLE IF NOT EXISTS collection_wallpapers (
                collection_id INTEGER NOT NULL,
                wallpaper_id TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (collection_id, wallpaper_id),
                FOREIGN KEY (collection_id) REFERENCES collections(id) ON DELETE CASCADE,
                FOREIGN KEY (wallpaper_id) REFERENCES wallpapers(id) ON DELETE CASCADE
            );

            -- Create indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_wallpapers_type ON wallpapers(wallpaper_type);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_source ON wallpapers(source_type);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_favorite ON wallpapers(favorite);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_name ON wallpapers(name);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_added ON wallpapers(added_at DESC);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_used ON wallpapers(last_used DESC);
            CREATE INDEX IF NOT EXISTS idx_wallpapers_rating ON wallpapers(rating DESC);
            CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);

            -- Full-text search table (FTS5)
            CREATE VIRTUAL TABLE IF NOT EXISTS wallpapers_fts USING fts5(
                name,
                title,
                author,
                description,
                tags,
                content='wallpapers',
                content_rowid='rowid'
            );

            -- Triggers to keep FTS index synchronized
            CREATE TRIGGER IF NOT EXISTS wallpapers_ai AFTER INSERT ON wallpapers BEGIN
                INSERT INTO wallpapers_fts(rowid, name, title, author, description, tags)
                VALUES (NEW.rowid, NEW.name, NEW.title, NEW.author, NEW.description, NEW.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS wallpapers_ad AFTER DELETE ON wallpapers BEGIN
                INSERT INTO wallpapers_fts(wallpapers_fts, rowid, name, title, author, description, tags)
                VALUES ('delete', OLD.rowid, OLD.name, OLD.title, OLD.author, OLD.description, OLD.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS wallpapers_au AFTER UPDATE ON wallpapers BEGIN
                INSERT INTO wallpapers_fts(wallpapers_fts, rowid, name, title, author, description, tags)
                VALUES ('delete', OLD.rowid, OLD.name, OLD.title, OLD.author, OLD.description, OLD.tags);
                INSERT INTO wallpapers_fts(rowid, name, title, author, description, tags)
                VALUES (NEW.rowid, NEW.name, NEW.title, NEW.author, NEW.description, NEW.tags);
            END;
        "#).context("Failed to initialize database schema")?;

        debug!("  ✓ Database schema initialized with FTS5");
        Ok(())
    }

    // ========== Wallpaper CRUD ==========

    /// Insert or update a wallpaper
    pub fn upsert_wallpaper(&self, item: &WallpaperItem) -> Result<()> {
        let conn = self.conn.write().unwrap();

        let tags_json = serde_json::to_string(&item.metadata.tags).unwrap_or_default();
        let (res_w, res_h) = item.metadata.resolution.unwrap_or((0, 0));

        conn.execute(
            r#"
            INSERT INTO wallpapers (
                id, name, source_path, source_type, wallpaper_type, thumbnail_path,
                title, author, description, tags, duration_secs, resolution_w, resolution_h,
                file_size, workshop_id, added_at, last_used
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(source_path) DO UPDATE SET
                name = excluded.name,
                wallpaper_type = excluded.wallpaper_type,
                thumbnail_path = excluded.thumbnail_path,
                title = excluded.title,
                author = excluded.author,
                description = excluded.description,
                tags = excluded.tags,
                duration_secs = excluded.duration_secs,
                resolution_w = excluded.resolution_w,
                resolution_h = excluded.resolution_h,
                file_size = excluded.file_size
            "#,
            params![
                item.id,
                item.name,
                item.source_path.to_string_lossy(),
                item.source_type.as_str(),
                item.wallpaper_type.as_str(),
                item.thumbnail_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                item.metadata.title,
                item.metadata.author,
                item.metadata.description,
                tags_json,
                item.metadata.duration_secs,
                res_w,
                res_h,
                item.metadata.file_size,
                item.metadata.workshop_id,
                item.added_at.to_rfc3339(),
                item.last_used.map(|d| d.to_rfc3339()),
            ],
        )?;

        Ok(())
    }

    /// Get wallpaper by ID
    pub fn get_wallpaper(&self, id: &str) -> Result<Option<WallpaperItem>> {
        let conn = self.conn.read().unwrap();

        conn.query_row(
            "SELECT * FROM wallpapers WHERE id = ?1",
            params![id],
            |row| self.row_to_wallpaper(row),
        )
        .optional()
        .context("Failed to query wallpaper")
    }

    /// Get wallpaper by path
    pub fn get_wallpaper_by_path(&self, path: &Path) -> Result<Option<WallpaperItem>> {
        let conn = self.conn.read().unwrap();

        conn.query_row(
            "SELECT * FROM wallpapers WHERE source_path = ?1",
            params![path.to_string_lossy()],
            |row| self.row_to_wallpaper(row),
        )
        .optional()
        .context("Failed to query wallpaper by path")
    }

    /// List all wallpapers
    pub fn list_wallpapers(&self, filter: &WallpaperFilter) -> Result<Vec<WallpaperItem>> {
        let conn = self.conn.read().unwrap();

        let mut sql = String::from("SELECT * FROM wallpapers WHERE 1=1");

        if filter.favorites_only {
            sql.push_str(" AND favorite = 1");
        }

        if let Some(ref type_filter) = filter.wallpaper_type {
            sql.push_str(&format!(" AND wallpaper_type = '{}'", type_filter.as_str()));
        }

        if let Some(ref source_filter) = filter.source_type {
            sql.push_str(&format!(" AND source_type = '{}'", source_filter.as_str()));
        }

        sql.push_str(match filter.sort_by {
            SortBy::Name => " ORDER BY name ASC",
            SortBy::DateAdded => " ORDER BY added_at DESC",
            SortBy::LastUsed => " ORDER BY last_used DESC NULLS LAST",
            SortBy::UseCount => " ORDER BY use_count DESC",
            SortBy::Rating => " ORDER BY rating DESC, name ASC",
            SortBy::Relevance => " ORDER BY name ASC", // Default for non-FTS queries
        });

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        let mut stmt = conn.prepare(&sql)?;
        let wallpapers = stmt
            .query_map([], |row| self.row_to_wallpaper(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(wallpapers)
    }

    /// Delete wallpaper by ID
    pub fn delete_wallpaper(&self, id: &str) -> Result<bool> {
        let conn = self.conn.write().unwrap();
        let rows = conn.execute("DELETE FROM wallpapers WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    /// Toggle favorite status
    pub fn toggle_favorite(&self, id: &str) -> Result<bool> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "UPDATE wallpapers SET favorite = NOT favorite WHERE id = ?1",
            params![id],
        )?;

        let new_state: bool = conn.query_row(
            "SELECT favorite FROM wallpapers WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        Ok(new_state)
    }

    /// Record wallpaper usage
    pub fn record_usage(&self, id: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "UPDATE wallpapers SET use_count = use_count + 1, last_used = ?2 WHERE id = ?1",
            params![id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Set wallpaper rating (0-5)
    pub fn set_rating(&self, id: &str, rating: u8) -> Result<()> {
        let rating = rating.min(5);
        let conn = self.conn.write().unwrap();
        conn.execute(
            "UPDATE wallpapers SET rating = ?2 WHERE id = ?1",
            params![id, rating],
        )?;
        Ok(())
    }

    /// Get wallpaper rating
    pub fn get_rating(&self, id: &str) -> Result<u8> {
        let conn = self.conn.read().unwrap();
        let rating: i32 = conn
            .query_row(
                "SELECT COALESCE(rating, 0) FROM wallpapers WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(rating as u8)
    }

    // ========== Full-Text Search ==========

    /// Search wallpapers using full-text search
    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<WallpaperItem>> {
        let conn = self.conn.read().unwrap();

        // Sanitize query for FTS5
        let sanitized = query.replace('"', "\"\"");
        let fts_query = format!("\"{}\"*", sanitized);

        let sql = format!(
            r#"
            SELECT w.* FROM wallpapers w
            JOIN wallpapers_fts ON w.rowid = wallpapers_fts.rowid
            WHERE wallpapers_fts MATCH ?1
            ORDER BY bm25(wallpapers_fts)
            {}
            "#,
            limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default()
        );

        let mut stmt = conn.prepare(&sql)?;
        let wallpapers = stmt
            .query_map(params![fts_query], |row| self.row_to_wallpaper(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(wallpapers)
    }

    /// Search wallpapers with advanced options
    pub fn search_advanced(&self, options: &SearchOptions) -> Result<Vec<WallpaperItem>> {
        let conn = self.conn.read().unwrap();

        let mut conditions = vec!["1=1".to_string()];

        // Full-text search condition
        if let Some(ref query) = options.query {
            let sanitized = query.replace('"', "\"\"");
            conditions.push(format!(
                "w.rowid IN (SELECT rowid FROM wallpapers_fts WHERE wallpapers_fts MATCH '\"{}\"*')",
                sanitized
            ));
        }

        // Type filter
        if let Some(ref wtype) = options.wallpaper_type {
            conditions.push(format!("w.wallpaper_type = '{}'", wtype.as_str()));
        }

        // Source filter
        if let Some(ref stype) = options.source_type {
            conditions.push(format!("w.source_type = '{}'", stype.as_str()));
        }

        // Favorites filter
        if options.favorites_only {
            conditions.push("w.favorite = 1".to_string());
        }

        // Rating filter
        if let Some(min_rating) = options.min_rating {
            conditions.push(format!("w.rating >= {}", min_rating));
        }

        // Tag filter (requires join)
        let tag_join = if !options.tags.is_empty() {
            let tag_names: Vec<String> = options
                .tags
                .iter()
                .map(|t| format!("'{}'", t.replace('\'', "''")))
                .collect();
            conditions.push(format!(
                "w.id IN (SELECT wt.wallpaper_id FROM wallpaper_tags wt JOIN tags t ON wt.tag_id = t.id WHERE t.name IN ({}))",
                tag_names.join(",")
            ));
            ""
        } else {
            ""
        };

        let order_by = match options.sort_by {
            SortBy::Name => "w.name ASC",
            SortBy::DateAdded => "w.added_at DESC",
            SortBy::LastUsed => "w.last_used DESC NULLS LAST",
            SortBy::UseCount => "w.use_count DESC",
            SortBy::Rating => "w.rating DESC, w.name ASC",
            SortBy::Relevance => "1", // FTS handles relevance
        };

        let sql = format!(
            "SELECT w.* FROM wallpapers w {} WHERE {} ORDER BY {} {}",
            tag_join,
            conditions.join(" AND "),
            order_by,
            options
                .limit
                .map(|l| format!("LIMIT {}", l))
                .unwrap_or_default()
        );

        let mut stmt = conn.prepare(&sql)?;
        let wallpapers = stmt
            .query_map([], |row| self.row_to_wallpaper(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(wallpapers)
    }

    // ========== Tags ==========

    /// Create a new tag
    pub fn create_tag(&self, name: &str, color: Option<&str>) -> Result<i64> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "INSERT INTO tags (name, color) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET color = excluded.color",
            params![name, color.unwrap_or("#666666")],
        )?;

        let tag_id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;

        Ok(tag_id)
    }

    /// Get all tags
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let conn = self.conn.read().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT t.id, t.name, t.color, COUNT(wt.wallpaper_id) as count
            FROM tags t
            LEFT JOIN wallpaper_tags wt ON t.id = wt.tag_id
            GROUP BY t.id
            ORDER BY count DESC, t.name ASC
            "#,
        )?;

        let tags = stmt
            .query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    wallpaper_count: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tags)
    }

    /// Add tag to wallpaper
    pub fn add_tag_to_wallpaper(&self, wallpaper_id: &str, tag_name: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();

        // Ensure tag exists
        conn.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![tag_name],
        )?;

        let tag_id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![tag_name],
            |row| row.get(0),
        )?;

        conn.execute(
            "INSERT OR IGNORE INTO wallpaper_tags (wallpaper_id, tag_id) VALUES (?1, ?2)",
            params![wallpaper_id, tag_id],
        )?;

        Ok(())
    }

    /// Remove tag from wallpaper
    pub fn remove_tag_from_wallpaper(&self, wallpaper_id: &str, tag_name: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            r#"
            DELETE FROM wallpaper_tags 
            WHERE wallpaper_id = ?1 
            AND tag_id = (SELECT id FROM tags WHERE name = ?2)
            "#,
            params![wallpaper_id, tag_name],
        )?;
        Ok(())
    }

    /// Get tags for a wallpaper
    pub fn get_wallpaper_tags(&self, wallpaper_id: &str) -> Result<Vec<Tag>> {
        let conn = self.conn.read().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT t.id, t.name, t.color, 1 as count
            FROM tags t
            JOIN wallpaper_tags wt ON t.id = wt.tag_id
            WHERE wt.wallpaper_id = ?1
            ORDER BY t.name
            "#,
        )?;

        let tags = stmt
            .query_map(params![wallpaper_id], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    wallpaper_count: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tags)
    }

    /// Delete a tag (removes from all wallpapers)
    pub fn delete_tag(&self, tag_name: &str) -> Result<bool> {
        let conn = self.conn.write().unwrap();
        let rows = conn.execute("DELETE FROM tags WHERE name = ?1", params![tag_name])?;
        Ok(rows > 0)
    }

    /// Rename a tag
    pub fn rename_tag(&self, old_name: &str, new_name: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "UPDATE tags SET name = ?2 WHERE name = ?1",
            params![old_name, new_name],
        )?;
        Ok(())
    }

    // ========== Collections ==========

    /// Create a new collection
    pub fn create_collection(&self, name: &str, description: Option<&str>) -> Result<i64> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "INSERT INTO collections (name, description) VALUES (?1, ?2)",
            params![name, description],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List all collections
    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let conn = self.conn.read().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.name, c.description, c.cover_wallpaper_id, 
                   COUNT(cw.wallpaper_id) as count, c.created_at
            FROM collections c
            LEFT JOIN collection_wallpapers cw ON c.id = cw.collection_id
            GROUP BY c.id
            ORDER BY c.name
            "#,
        )?;

        let collections = stmt
            .query_map([], |row| {
                Ok(Collection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    cover_wallpaper_id: row.get(3)?,
                    wallpaper_count: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(collections)
    }

    /// Add wallpaper to collection
    pub fn add_to_collection(&self, collection_id: i64, wallpaper_id: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();

        // Get next position
        let position: i32 = conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM collection_wallpapers WHERE collection_id = ?1",
            params![collection_id],
            |row| row.get(0),
        )?;

        conn.execute(
            "INSERT OR IGNORE INTO collection_wallpapers (collection_id, wallpaper_id, position) VALUES (?1, ?2, ?3)",
            params![collection_id, wallpaper_id, position],
        )?;
        Ok(())
    }

    /// Remove wallpaper from collection
    pub fn remove_from_collection(&self, collection_id: i64, wallpaper_id: &str) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "DELETE FROM collection_wallpapers WHERE collection_id = ?1 AND wallpaper_id = ?2",
            params![collection_id, wallpaper_id],
        )?;
        Ok(())
    }

    /// Get wallpapers in collection
    pub fn get_collection_wallpapers(&self, collection_id: i64) -> Result<Vec<WallpaperItem>> {
        let conn = self.conn.read().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT w.* FROM wallpapers w
            JOIN collection_wallpapers cw ON w.id = cw.wallpaper_id
            WHERE cw.collection_id = ?1
            ORDER BY cw.position
            "#,
        )?;

        let wallpapers = stmt
            .query_map(params![collection_id], |row| self.row_to_wallpaper(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(wallpapers)
    }

    /// Delete collection
    pub fn delete_collection(&self, collection_id: i64) -> Result<bool> {
        let conn = self.conn.write().unwrap();
        let rows = conn.execute(
            "DELETE FROM collections WHERE id = ?1",
            params![collection_id],
        )?;
        Ok(rows > 0)
    }

    // ========== Folders ==========

    /// Add a library folder
    pub fn add_folder(&self, path: &Path, recursive: bool) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO folders (path, scan_recursive) VALUES (?1, ?2)",
            params![path.to_string_lossy(), recursive as i32],
        )?;
        Ok(())
    }

    /// List all library folders
    pub fn list_folders(&self) -> Result<Vec<LibraryFolder>> {
        let conn = self.conn.read().unwrap();
        let mut stmt =
            conn.prepare("SELECT path, enabled, scan_recursive, last_scanned_at FROM folders")?;

        let folders = stmt
            .query_map([], |row| {
                Ok(LibraryFolder {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    enabled: row.get::<_, i32>(1)? != 0,
                    recursive: row.get::<_, i32>(2)? != 0,
                    last_scanned: row.get::<_, Option<String>>(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(folders)
    }

    /// Remove a library folder
    pub fn remove_folder(&self, path: &Path) -> Result<bool> {
        let conn = self.conn.write().unwrap();
        let rows = conn.execute(
            "DELETE FROM folders WHERE path = ?1",
            params![path.to_string_lossy()],
        )?;
        Ok(rows > 0)
    }

    // ========== Thumbnails ==========

    /// Store thumbnail data
    pub fn store_thumbnail(
        &self,
        wallpaper_id: &str,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<()> {
        let conn = self.conn.write().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO thumbnails (wallpaper_id, data, width, height) VALUES (?1, ?2, ?3, ?4)",
            params![wallpaper_id, data, width, height],
        )?;
        Ok(())
    }

    /// Get thumbnail data
    pub fn get_thumbnail(&self, wallpaper_id: &str) -> Result<Option<ThumbnailData>> {
        let conn = self.conn.read().unwrap();
        conn.query_row(
            "SELECT data, width, height FROM thumbnails WHERE wallpaper_id = ?1",
            params![wallpaper_id],
            |row| {
                Ok(ThumbnailData {
                    data: row.get(0)?,
                    width: row.get(1)?,
                    height: row.get(2)?,
                })
            },
        )
        .optional()
        .context("Failed to query thumbnail")
    }

    // ========== Stats ==========

    /// Get library statistics
    pub fn get_stats(&self) -> Result<LibraryStats> {
        let conn = self.conn.read().unwrap();

        let total: i64 = conn.query_row("SELECT COUNT(*) FROM wallpapers", [], |row| row.get(0))?;

        let favorites: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallpapers WHERE favorite = 1",
            [],
            |row| row.get(0),
        )?;

        let videos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallpapers WHERE wallpaper_type = 'video'",
            [],
            |row| row.get(0),
        )?;

        let images: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallpapers WHERE wallpaper_type = 'image'",
            [],
            |row| row.get(0),
        )?;

        let total_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(file_size), 0) FROM wallpapers",
            [],
            |row| row.get(0),
        )?;

        let tags_count: i64 = conn.query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))?;

        let collections_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))?;

        Ok(LibraryStats {
            total_wallpapers: total as usize,
            favorites: favorites as usize,
            videos: videos as usize,
            images: images as usize,
            total_size_bytes: total_size as u64,
            tags_count: tags_count as usize,
            collections_count: collections_count as usize,
        })
    }

    // ========== Helpers ==========

    fn row_to_wallpaper(&self, row: &rusqlite::Row) -> rusqlite::Result<WallpaperItem> {
        let id: String = row.get("id")?;
        let name: String = row.get("name")?;
        let source_path: String = row.get("source_path")?;
        let source_type_str: String = row.get("source_type")?;
        let wallpaper_type_str: String = row.get("wallpaper_type")?;
        let thumbnail_path: Option<String> = row.get("thumbnail_path")?;

        let title: Option<String> = row.get("title")?;
        let author: Option<String> = row.get("author")?;
        let description: Option<String> = row.get("description")?;
        let tags_json: Option<String> = row.get("tags")?;
        let duration_secs: Option<f64> = row.get("duration_secs")?;
        let res_w: Option<u32> = row.get("resolution_w")?;
        let res_h: Option<u32> = row.get("resolution_h")?;
        let file_size: Option<u64> = row.get("file_size")?;
        let workshop_id: Option<u64> = row.get("workshop_id")?;
        let added_at_str: String = row.get("added_at")?;
        let last_used_str: Option<String> = row.get("last_used")?;

        let tags: Vec<String> = tags_json
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();

        let resolution = match (res_w, res_h) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
            _ => None,
        };

        let metadata = WallpaperMetadata {
            title,
            author,
            description,
            tags,
            duration_secs,
            resolution,
            file_size,
            workshop_id,
        };

        let added_at = DateTime::parse_from_rfc3339(&added_at_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let last_used = last_used_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|d| d.with_timezone(&Utc))
                .ok()
        });

        Ok(WallpaperItem {
            id,
            name,
            source_path: PathBuf::from(source_path),
            source_type: str_to_source_type(&source_type_str),
            wallpaper_type: str_to_wallpaper_type(&wallpaper_type_str),
            thumbnail_path: thumbnail_path.map(PathBuf::from),
            metadata,
            added_at,
            last_used,
        })
    }
}

// ========== Types ==========

/// Filter options for wallpaper queries
#[derive(Debug, Clone, Default)]
pub struct WallpaperFilter {
    pub favorites_only: bool,
    pub wallpaper_type: Option<WallpaperType>,
    pub source_type: Option<SourceType>,
    pub sort_by: SortBy,
    pub limit: Option<usize>,
}

/// Advanced search options
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Full-text search query
    pub query: Option<String>,
    /// Filter by wallpaper type
    pub wallpaper_type: Option<WallpaperType>,
    /// Filter by source type
    pub source_type: Option<SourceType>,
    /// Filter favorites only
    pub favorites_only: bool,
    /// Filter by minimum rating (0-5)
    pub min_rating: Option<u8>,
    /// Filter by tags (match any)
    pub tags: Vec<String>,
    /// Sort order
    pub sort_by: SortBy,
    /// Maximum results
    pub limit: Option<usize>,
}

/// Sort order for wallpaper queries
#[derive(Debug, Clone, Copy, Default)]
pub enum SortBy {
    #[default]
    Name,
    DateAdded,
    LastUsed,
    UseCount,
    Rating,
    Relevance,
}

/// Library folder info
#[derive(Debug, Clone)]
pub struct LibraryFolder {
    pub path: PathBuf,
    pub enabled: bool,
    pub recursive: bool,
    pub last_scanned: Option<String>,
}

/// Tag information
#[derive(Debug, Clone)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub wallpaper_count: i64,
}

/// Collection information
#[derive(Debug, Clone)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_wallpaper_id: Option<String>,
    pub wallpaper_count: i64,
    pub created_at: String,
}

/// Thumbnail data
#[derive(Debug, Clone)]
pub struct ThumbnailData {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Library statistics
#[derive(Debug, Clone, Default)]
pub struct LibraryStats {
    pub total_wallpapers: usize,
    pub favorites: usize,
    pub videos: usize,
    pub images: usize,
    pub total_size_bytes: u64,
    pub tags_count: usize,
    pub collections_count: usize,
}

// ========== Conversion helpers ==========

fn str_to_wallpaper_type(s: &str) -> WallpaperType {
    match s {
        "video" => WallpaperType::Video,
        "image" => WallpaperType::Image,
        "scene" => WallpaperType::Scene,
        "gif" => WallpaperType::Gif,
        _ => WallpaperType::Video,
    }
}

fn str_to_source_type(s: &str) -> SourceType {
    match s {
        "local_file" => SourceType::LocalFile,
        "local_dir" => SourceType::LocalDirectory,
        "workshop" => SourceType::SteamWorkshop,
        _ => SourceType::LocalFile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (LibraryDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = LibraryDatabase::open(&db_path).unwrap();
        (db, temp_dir)
    }

    fn create_test_wallpaper(name: &str, wtype: WallpaperType) -> WallpaperItem {
        let mut item = WallpaperItem::new(
            PathBuf::from(format!("/tmp/{}.mp4", name)),
            name.to_string(),
            SourceType::LocalFile,
            wtype,
        );
        item.metadata.title = Some(format!("{} Title", name));
        item.metadata.author = Some("Test Author".to_string());
        item.metadata.description = Some(format!("Description for {}", name));
        item
    }

    #[test]
    fn test_database_creation() {
        let (db, _temp) = create_test_db();
        let stats = db.get_stats().unwrap();
        assert_eq!(stats.total_wallpapers, 0);
        assert_eq!(stats.tags_count, 0);
        assert_eq!(stats.collections_count, 0);
    }

    #[test]
    fn test_wallpaper_crud() {
        let (db, _temp) = create_test_db();

        let item = WallpaperItem::new(
            PathBuf::from("/tmp/test.mp4"),
            "Test Video".to_string(),
            SourceType::LocalFile,
            WallpaperType::Video,
        );

        // Insert
        db.upsert_wallpaper(&item).unwrap();

        // Query
        let retrieved = db.get_wallpaper(&item.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "Test Video");
        assert_eq!(retrieved.wallpaper_type, WallpaperType::Video);

        // Toggle favorite
        let is_fav = db.toggle_favorite(&item.id).unwrap();
        assert!(is_fav);

        // Delete
        let deleted = db.delete_wallpaper(&item.id).unwrap();
        assert!(deleted);

        // Verify deleted
        let none = db.get_wallpaper(&item.id).unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn test_library_folders() {
        let (db, _temp) = create_test_db();

        db.add_folder(Path::new("/home/user/wallpapers"), true)
            .unwrap();
        db.add_folder(Path::new("/home/user/videos"), false)
            .unwrap();

        let folders = db.list_folders().unwrap();
        assert_eq!(folders.len(), 2);
        assert!(
            folders
                .iter()
                .any(|f| f.path.to_string_lossy().contains("wallpapers"))
        );

        db.remove_folder(Path::new("/home/user/wallpapers"))
            .unwrap();
        let folders = db.list_folders().unwrap();
        assert_eq!(folders.len(), 1);
    }

    #[test]
    fn test_full_text_search() {
        let (db, _temp) = create_test_db();

        // Insert wallpapers with searchable content
        let item1 = create_test_wallpaper("nature_sunset", WallpaperType::Video);
        let item2 = create_test_wallpaper("city_night", WallpaperType::Video);
        let item3 = create_test_wallpaper("ocean_waves", WallpaperType::Image);

        db.upsert_wallpaper(&item1).unwrap();
        db.upsert_wallpaper(&item2).unwrap();
        db.upsert_wallpaper(&item3).unwrap();

        // Search for "nature"
        let results = db.search("nature", None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name.contains("nature"));

        // Search for "city"
        let results = db.search("city", None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name.contains("city"));
    }

    #[test]
    fn test_tags() {
        let (db, _temp) = create_test_db();

        let item = create_test_wallpaper("tagged_wallpaper", WallpaperType::Video);
        db.upsert_wallpaper(&item).unwrap();

        // Create tags
        db.create_tag("nature", Some("#00ff00")).unwrap();
        db.create_tag("sunset", Some("#ff6600")).unwrap();

        // Add tags to wallpaper
        db.add_tag_to_wallpaper(&item.id, "nature").unwrap();
        db.add_tag_to_wallpaper(&item.id, "sunset").unwrap();

        // Get wallpaper tags
        let tags = db.get_wallpaper_tags(&item.id).unwrap();
        assert_eq!(tags.len(), 2);

        // List all tags
        let all_tags = db.list_tags().unwrap();
        assert_eq!(all_tags.len(), 2);

        // Remove tag
        db.remove_tag_from_wallpaper(&item.id, "sunset").unwrap();
        let tags = db.get_wallpaper_tags(&item.id).unwrap();
        assert_eq!(tags.len(), 1);

        // Delete tag
        db.delete_tag("nature").unwrap();
        let tags = db.get_wallpaper_tags(&item.id).unwrap();
        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_collections() {
        let (db, _temp) = create_test_db();

        let item1 = create_test_wallpaper("collection_wp1", WallpaperType::Video);
        let item2 = create_test_wallpaper("collection_wp2", WallpaperType::Image);
        db.upsert_wallpaper(&item1).unwrap();
        db.upsert_wallpaper(&item2).unwrap();

        // Create collection
        let collection_id = db
            .create_collection("My Favorites", Some("Best wallpapers"))
            .unwrap();
        assert!(collection_id > 0);

        // Add wallpapers to collection
        db.add_to_collection(collection_id, &item1.id).unwrap();
        db.add_to_collection(collection_id, &item2.id).unwrap();

        // Get collection wallpapers
        let wallpapers = db.get_collection_wallpapers(collection_id).unwrap();
        assert_eq!(wallpapers.len(), 2);

        // List collections
        let collections = db.list_collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].wallpaper_count, 2);

        // Remove from collection
        db.remove_from_collection(collection_id, &item1.id).unwrap();
        let wallpapers = db.get_collection_wallpapers(collection_id).unwrap();
        assert_eq!(wallpapers.len(), 1);

        // Delete collection
        db.delete_collection(collection_id).unwrap();
        let collections = db.list_collections().unwrap();
        assert_eq!(collections.len(), 0);
    }

    #[test]
    fn test_rating() {
        let (db, _temp) = create_test_db();

        let item = create_test_wallpaper("rated_wallpaper", WallpaperType::Video);
        db.upsert_wallpaper(&item).unwrap();

        // Set rating
        db.set_rating(&item.id, 4).unwrap();
        assert_eq!(db.get_rating(&item.id).unwrap(), 4);

        // Rating cap at 5
        db.set_rating(&item.id, 10).unwrap();
        assert_eq!(db.get_rating(&item.id).unwrap(), 5);
    }

    #[test]
    fn test_advanced_search() {
        let (db, _temp) = create_test_db();

        let item1 = create_test_wallpaper("video_nature", WallpaperType::Video);
        let item2 = create_test_wallpaper("video_city", WallpaperType::Video);
        let item3 = create_test_wallpaper("image_sunset", WallpaperType::Image);

        db.upsert_wallpaper(&item1).unwrap();
        db.upsert_wallpaper(&item2).unwrap();
        db.upsert_wallpaper(&item3).unwrap();

        // Set ratings
        db.set_rating(&item1.id, 5).unwrap();
        db.set_rating(&item2.id, 3).unwrap();

        // Toggle favorites
        db.toggle_favorite(&item1.id).unwrap();

        // Advanced search: videos only
        let options = SearchOptions {
            wallpaper_type: Some(WallpaperType::Video),
            ..Default::default()
        };
        let results = db.search_advanced(&options).unwrap();
        assert_eq!(results.len(), 2);

        // Advanced search: favorites + min rating
        let options = SearchOptions {
            favorites_only: true,
            min_rating: Some(4),
            ..Default::default()
        };
        let results = db.search_advanced(&options).unwrap();
        assert_eq!(results.len(), 1);
    }
}
