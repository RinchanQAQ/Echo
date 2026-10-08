//! 存储层：SQLite（rusqlite，bundled 模式，不依赖系统 sqlite3）。
//!
//! 连接用 `Arc<Mutex<Connection>>` 包起来，扫描线程与 UI 线程共享同一个句柄。
//! 约定：**绝不在持锁期间做长耗时 I/O**（文件读取、图片解码都在锁外完成）。

// 这里是一个较完整的存储接口，其中一部分方法（track_by_id、clear_tracks、
// cover_count、remove_scan_root 等）当前只被测试使用，保留它们是刻意的：
// 它们构成存储层对外契约的完整性，后续功能会用到。
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::{CoverArt, Track};

/// 当前 schema 版本，写入 `PRAGMA user_version`。
const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS covers(
    hash TEXT PRIMARY KEY,
    mime TEXT NOT NULL,
    data BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS tracks(
    id            INTEGER PRIMARY KEY,
    path          TEXT    NOT NULL UNIQUE,
    title         TEXT    NOT NULL DEFAULT '',
    artist        TEXT    NOT NULL DEFAULT '',
    album         TEXT    NOT NULL DEFAULT '',
    album_artist  TEXT    NOT NULL DEFAULT '',
    track_no      INTEGER,
    disc_no       INTEGER,
    year          INTEGER,
    genre         TEXT    NOT NULL DEFAULT '',
    duration_ms   INTEGER NOT NULL DEFAULT 0,
    sample_rate   INTEGER,
    bitrate       INTEGER,
    channels      INTEGER,
    cover_hash    TEXT REFERENCES covers(hash),
    file_size     INTEGER NOT NULL DEFAULT 0,
    mtime         INTEGER NOT NULL DEFAULT 0,
    added_at      INTEGER NOT NULL DEFAULT 0,
    last_seen_scan INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scan_roots(
    id       INTEGER PRIMARY KEY,
    path     TEXT NOT NULL UNIQUE,
    added_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_tracks_album
    ON tracks(album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_tracks_artist
    ON tracks(artist COLLATE NOCASE);
"#;

/// 文件签名：用于「未变更则跳过」的快速路径。
pub type FileSignature = (u64, i64);

/// 数据库句柄（可跨线程克隆）。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// 打开（或新建）指定路径的数据库。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("无法创建数据目录 {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("无法打开数据库 {}", path.display()))?;
        Self::from_connection(conn)
    }

    /// 内存数据库，仅测试用。
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        // WAL 让读写并发更顺滑；busy_timeout 容忍第二个实例短暂占锁。
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .context("初始化 SQLite PRAGMA 失败")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 取连接锁。锁中毒说明其它线程 panic 过，这里直接恢复内部值继续用。
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 幂等迁移：只在 `user_version` 落后时执行。
    fn migrate(&self) -> Result<()> {
        let conn = self.lock();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(SCHEMA_V1).context("创建数据库表失败")?;
            // PRAGMA 不支持绑定参数，此处是编译期常量，无注入风险。
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        Ok(())
    }

    // ---------- 曲目 ----------

    /// 按 艺术家 → 专辑 → 碟号 → 音轨号 → 标题 排序取出全部曲目。
    pub fn all_tracks_sorted(&self) -> Result<Vec<Track>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, artist, album, album_artist,
                    track_no, disc_no, year, genre, duration_ms,
                    sample_rate, bitrate, channels, cover_hash, file_size, mtime
             FROM tracks
             ORDER BY artist COLLATE NOCASE,
                      album COLLATE NOCASE,
                      disc_no,
                      track_no,
                      title COLLATE NOCASE,
                      path COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_track)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 单曲查询，用于「已删除文件」的兜底校验。
    pub fn track_by_id(&self, id: i64) -> Result<Option<Track>> {
        let conn = self.lock();
        let t = conn
            .query_row(
                "SELECT id, path, title, artist, album, album_artist,
                        track_no, disc_no, year, genre, duration_ms,
                        sample_rate, bitrate, channels, cover_hash, file_size, mtime
                 FROM tracks WHERE id = ?1",
                [id],
                row_to_track,
            )
            .optional()?;
        Ok(t)
    }

    /// 读取某路径已记录的 (大小, mtime, 封面哈希)。
    ///
    /// 扫描时用它判断文件是否变更：未变更就完全不必解析标签，
    /// 这是「重扫很快」的关键。
    pub fn existing_signature(
        &self,
        path: &str,
    ) -> Result<Option<(FileSignature, Option<String>)>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT file_size, mtime, cover_hash FROM tracks WHERE path = ?1",
                [path],
                |r| {
                    Ok((
                        // 夹到 0：负数理论上不该出现，但直接 `as u64` 会绕回成
                        // 一个巨大的值，导致「文件已变更」的判断永远成立。
                        (r.get::<_, i64>(0)?.max(0) as u64, r.get::<_, i64>(1)?),
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn track_count(&self) -> Result<usize> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 插入或更新一首曲目，并标记它属于本次扫描（`last_seen_scan`）。
    ///
    /// `ON CONFLICT(path) DO UPDATE` 保证同一文件**永远不会重复入库**。
    /// 返回 `true` 表示这是新插入的行。
    pub fn upsert_track(&self, meta: &crate::domain::TrackMeta, scan_id: i64) -> Result<bool> {
        let conn = self.lock();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM tracks WHERE path = ?1",
                [path_str(&meta.path)],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);

        let cover_hash = meta.cover.as_ref().map(|c| c.hash.as_str());
        // 注意 ?17 与 ?18 的区别：added_at 是「首次入库时间」，
        // last_seen_scan 必须绑定本次扫描编号，否则收尾的
        // delete_tracks_not_in_scan 会把所有曲目都当成陈旧数据删掉。
        conn.execute(
            "INSERT INTO tracks
                (path, title, artist, album, album_artist, track_no, disc_no, year, genre,
                 duration_ms, sample_rate, bitrate, channels, cover_hash,
                 file_size, mtime, added_at, last_seen_scan)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(path) DO UPDATE SET
                title         = excluded.title,
                artist        = excluded.artist,
                album         = excluded.album,
                album_artist  = excluded.album_artist,
                track_no      = excluded.track_no,
                disc_no       = excluded.disc_no,
                year          = excluded.year,
                genre         = excluded.genre,
                duration_ms   = excluded.duration_ms,
                sample_rate   = excluded.sample_rate,
                bitrate       = excluded.bitrate,
                channels      = excluded.channels,
                cover_hash    = excluded.cover_hash,
                file_size     = excluded.file_size,
                mtime         = excluded.mtime,
                last_seen_scan = excluded.last_seen_scan",
            params![
                path_str(&meta.path),
                meta.title,
                meta.artist,
                meta.album,
                meta.album_artist,
                meta.track_no,
                meta.disc_no,
                meta.year,
                meta.genre,
                meta.duration_ms as i64,
                meta.sample_rate,
                meta.bitrate,
                meta.channels,
                cover_hash,
                meta.file_size as i64,
                meta.mtime,
                now_unix(),
                scan_id,
            ],
        )?;
        Ok(!exists)
    }

    /// 把一批路径标记为「本次扫描已见到」，但不改动它们的元数据。
    ///
    /// 这是「未变更则跳过」路径的必需配套：跳过的文件不会被 upsert，
    /// 如果不同步刷新 `last_seen_scan`，收尾的 `delete_tracks_not_in_scan`
    /// 会把它们全部当作陈旧数据删除，曲库会莫名其妙清空。
    pub fn touch_tracks(&self, paths: &[String], scan_id: i64) -> Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut n = 0usize;
        {
            let mut stmt = tx.prepare("UPDATE tracks SET last_seen_scan = ?1 WHERE path = ?2")?;
            for p in paths {
                n += stmt.execute(params![scan_id, p])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// 删除本次扫描未再见到的曲目（文件被移走或删除）。返回删除条数。
    pub fn delete_tracks_not_in_scan(&self, scan_id: i64) -> Result<usize> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM tracks WHERE last_seen_scan != ?1", [scan_id])?;
        Ok(n)
    }

    /// 为「文件存在但无法解析」的曲目写一条占位记录，只更新签名与归属。
    ///
    /// 有了它，损坏/不支持的文件既会出现在曲库里（用户能看到并对它做处理），
    /// 又不会在收尾的「清理未见到行」步骤里被误删。
    pub fn upsert_stub(&self, path: &Path, file_size: u64, mtime: i64, scan_id: i64) -> Result<()> {
        let conn = self.lock();
        // 注意：不要写成 `VALUES (?1,?2,?3,?4,?4) ... last_seen_scan = ?5`。
        // 实测 SQLite 在这种「插入分支复用 ?4、更新分支再引用 ?5」的写法下，
        // 会把 last_seen_scan 写成 ?4（也就是 added_at）的值，
        // 导致占位行被收尾的清理逻辑当作陈旧数据删除。
        // 这里让两个分支都各自显式绑定，语义最清晰。
        conn.execute(
            "INSERT INTO tracks (path, file_size, mtime, added_at, last_seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
                file_size      = excluded.file_size,
                mtime          = excluded.mtime,
                last_seen_scan = ?5",
            params![path_str(path), file_size as i64, mtime, now_unix(), scan_id],
        )?;
        Ok(())
    }

    /// 清空整库（用于「重新扫描」时彻底重建）。
    pub fn clear_tracks(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch("DELETE FROM tracks;")?;
        Ok(())
    }

    // ---------- 封面 ----------

    /// 封面按内容哈希去重；同一个封面重复写入时直接忽略。
    pub fn put_cover(&self, cover: &CoverArt) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO covers(hash, mime, data) VALUES (?1, ?2, ?3)",
            params![cover.hash, cover.mime, cover.bytes],
        )?;
        Ok(())
    }

    /// 读取封面字节。
    pub fn get_cover(&self, hash: &str) -> Result<Option<(String, Vec<u8>)>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT mime, data FROM covers WHERE hash = ?1",
                [hash],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn cover_count(&self) -> Result<usize> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM covers", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 清理已无曲目引用的封面。
    pub fn prune_orphan_covers(&self) -> Result<usize> {
        let conn = self.lock();
        let n = conn.execute(
            "DELETE FROM covers WHERE hash NOT IN (SELECT cover_hash FROM tracks WHERE cover_hash IS NOT NULL)",
            [],
        )?;
        Ok(n)
    }

    // ---------- 扫描目录 ----------

    /// 记录一个扫描目录（重复添加不报错、不重复）。
    pub fn add_scan_root(&self, path: &Path) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO scan_roots(path, added_at) VALUES (?1, ?2)",
            params![path_str(path), now_unix()],
        )?;
        Ok(())
    }

    pub fn scan_roots(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT path FROM scan_roots ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn remove_scan_root(&self, path: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM scan_roots WHERE path = ?1", [path])?;
        Ok(())
    }

    /// 生成本次扫描的编号。
    ///
    /// 用「已有记录里的最大值 + 1」而不是时间戳：单调递增能保证
    /// 同一秒内连续两次扫描也不会撞号，从而让「本次未见到」的判定始终准确。
    pub fn begin_scan(&self) -> i64 {
        let conn = self.lock();
        conn.query_row(
            "SELECT COALESCE(MAX(last_seen_scan), 0) + 1 FROM tracks",
            [],
            |r| r.get(0),
        )
        .unwrap_or(1)
    }
}

/// 路径转字符串。Windows 上 Rust 的路径本就是有效 UTF-8（WTF-8 除外），
/// 用 lossy 转换保证永不 panic。
fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn row_to_track(r: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        path: std::path::PathBuf::from(r.get::<_, String>(1)?),
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        album_artist: r.get(5)?,
        track_no: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        disc_no: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        year: r.get::<_, Option<i64>>(8)?.map(|v| v as u16),
        genre: r.get(9)?,
        duration_ms: r.get::<_, i64>(10)?.max(0) as u64,
        sample_rate: r.get::<_, Option<i64>>(11)?.map(|v| v as u32),
        bitrate: r.get::<_, Option<i64>>(12)?.map(|v| v as u32),
        channels: r.get::<_, Option<i64>>(13)?.map(|v| v as u8),
        cover_hash: r.get(14)?,
        file_size: r.get::<_, i64>(15)?.max(0) as u64,
        mtime: r.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CoverArt, TrackMeta};
    use std::path::PathBuf;

    fn meta(path: &str, size: u64, mtime: i64) -> TrackMeta {
        TrackMeta {
            path: PathBuf::from(path),
            title: "声波测试".into(),
            artist: "艺术家".into(),
            album: "专辑 A".into(),
            album_artist: "合辑".into(),
            track_no: Some(3),
            disc_no: Some(1),
            year: Some(2024),
            genre: "摇滚".into(),
            duration_ms: 187_000,
            sample_rate: Some(44_100),
            bitrate: Some(320),
            channels: Some(2),
            cover: None,
            file_size: size,
            mtime,
        }
    }

    #[test]
    fn upsert_stub_records_scan_id() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        db.upsert_stub(Path::new("C:/m/broken.mp3"), 19, 555, scan)
            .unwrap();

        // 关键断言：占位行的 last_seen_scan 必须等于本次扫描编号，
        // 否则收尾的 delete_tracks_not_in_scan 会把它当陈旧数据删掉。
        assert_eq!(db.track_count().unwrap(), 1);
        assert_eq!(db.delete_tracks_not_in_scan(scan).unwrap(), 0);

        // 更新分支同样要刷新 last_seen_scan。
        let scan2 = db.begin_scan();
        db.upsert_stub(Path::new("C:/m/broken.mp3"), 19, 555, scan2)
            .unwrap();
        assert_eq!(
            db.delete_tracks_not_in_scan(scan2).unwrap(),
            0,
            "重复写入占位行后也不应被清理"
        );
        assert_eq!(db.track_count().unwrap(), 1);
    }

    #[test]
    fn touch_tracks_keeps_unchanged_rows_alive() {
        let db = Db::open_in_memory().unwrap();
        let scan1 = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 1, 1), scan1).unwrap();
        db.upsert_track(&meta("C:/m/b.mp3", 1, 1), scan1).unwrap();

        // 第二次扫描：a.mp3 走快速路径被「标记为已见到」，b.mp3 没再出现。
        let scan2 = db.begin_scan();
        let touched = db.touch_tracks(&["C:/m/a.mp3".to_string()], scan2).unwrap();
        assert_eq!(touched, 1);

        assert_eq!(db.delete_tracks_not_in_scan(scan2).unwrap(), 1);
        let all = db.all_tracks_sorted().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].path.ends_with("a.mp3"), "被 touch 的行必须保留");
    }

    #[test]
    fn touch_tracks_handles_empty_input() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.touch_tracks(&[], 1).unwrap(), 0);
    }

    #[test]
    fn migration_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        // 再跑一次迁移不应报错（表已存在）。
        db.migrate().unwrap();
        db.migrate().unwrap();
        let v: i64 = db
            .lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn upsert_same_path_yields_single_row() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        assert!(db.upsert_track(&meta("C:/m/a.mp3", 10, 100), scan).unwrap());
        // 第二次写入应当是更新而非新增。
        assert!(!db.upsert_track(&meta("C:/m/a.mp3", 10, 100), scan).unwrap());
        assert_eq!(db.track_count().unwrap(), 1);
    }

    #[test]
    fn upsert_updates_changed_fields() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 10, 100), scan).unwrap();

        let mut changed = meta("C:/m/a.mp3", 20, 200);
        changed.title = "新标题".into();
        changed.duration_ms = 5_000;
        db.upsert_track(&changed, scan).unwrap();

        let all = db.all_tracks_sorted().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "新标题");
        assert_eq!(all[0].duration_ms, 5_000);
        assert_eq!(all[0].file_size, 20);
    }

    #[test]
    fn utf8_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        let mut m = meta("C:/音乐/测试曲.mp3", 1, 1);
        m.title = "声波测试".into();
        m.artist = "张伟".into();
        db.upsert_track(&m, scan).unwrap();

        let all = db.all_tracks_sorted().unwrap();
        assert_eq!(all[0].title, "声波测试");
        assert_eq!(all[0].artist, "张伟");
        assert_eq!(all[0].path, PathBuf::from("C:/音乐/测试曲.mp3"));
    }

    #[test]
    fn optional_fields_can_be_null() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        let mut m = meta("C:/m/b.mp3", 1, 1);
        m.track_no = None;
        m.disc_no = None;
        m.year = None;
        m.sample_rate = None;
        m.bitrate = None;
        m.channels = None;
        db.upsert_track(&m, scan).unwrap();

        let t = &db.all_tracks_sorted().unwrap()[0];
        assert_eq!(t.track_no, None);
        assert_eq!(t.year, None);
        assert_eq!(t.sample_rate, None);
        assert_eq!(t.channels, None);
    }

    #[test]
    fn existing_signature_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.existing_signature("C:/m/a.mp3").unwrap().is_none());

        let scan = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 4096, 12345), scan)
            .unwrap();

        let (sig, cover) = db.existing_signature("C:/m/a.mp3").unwrap().unwrap();
        assert_eq!(sig, (4096, 12345));
        assert_eq!(cover, None);
    }

    #[test]
    fn covers_are_deduplicated_by_hash() {
        let db = Db::open_in_memory().unwrap();
        let art = CoverArt {
            hash: "abc123".into(),
            mime: "image/jpeg".into(),
            bytes: vec![1, 2, 3, 4],
        };
        db.put_cover(&art).unwrap();
        db.put_cover(&art).unwrap(); // 重复写入应被忽略
        assert_eq!(db.cover_count().unwrap(), 1);

        let (mime, data) = db.get_cover("abc123").unwrap().unwrap();
        assert_eq!(mime, "image/jpeg");
        assert_eq!(data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn track_links_to_cover() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        let art = CoverArt {
            hash: "cover1".into(),
            mime: "image/png".into(),
            bytes: vec![9, 9],
        };
        db.put_cover(&art).unwrap();

        let mut m = meta("C:/m/c.mp3", 1, 1);
        m.cover = Some(art);
        db.upsert_track(&m, scan).unwrap();

        let t = &db.all_tracks_sorted().unwrap()[0];
        assert_eq!(t.cover_hash.as_deref(), Some("cover1"));
        let (_, sig_cover) = db.existing_signature("C:/m/c.mp3").unwrap().unwrap();
        assert_eq!(sig_cover.as_deref(), Some("cover1"));
    }

    #[test]
    fn delete_removes_only_unseen_rows() {
        let db = Db::open_in_memory().unwrap();
        let scan1 = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 1, 1), scan1).unwrap();
        db.upsert_track(&meta("C:/m/b.mp3", 1, 1), scan1).unwrap();

        // 第二次扫描只见到 a.mp3。
        let scan2 = scan1 + 1;
        db.upsert_track(&meta("C:/m/a.mp3", 1, 1), scan2).unwrap();
        let removed = db.delete_tracks_not_in_scan(scan2).unwrap();

        assert_eq!(removed, 1);
        let all = db.all_tracks_sorted().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].path.ends_with("a.mp3"));
    }

    #[test]
    fn orphan_covers_are_pruned() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        let used = CoverArt {
            hash: "used".into(),
            mime: "image/png".into(),
            bytes: vec![1],
        };
        let orphan = CoverArt {
            hash: "orphan".into(),
            mime: "image/png".into(),
            bytes: vec![2],
        };
        db.put_cover(&used).unwrap();
        db.put_cover(&orphan).unwrap();

        let mut m = meta("C:/m/a.mp3", 1, 1);
        m.cover = Some(used);
        db.upsert_track(&m, scan).unwrap();

        assert_eq!(db.prune_orphan_covers().unwrap(), 1);
        assert_eq!(db.cover_count().unwrap(), 1);
        assert!(db.get_cover("used").unwrap().is_some());
        assert!(db.get_cover("orphan").unwrap().is_none());
    }

    #[test]
    fn scan_roots_are_idempotent() {
        let db = Db::open_in_memory().unwrap();
        db.add_scan_root(Path::new("D:/音乐")).unwrap();
        db.add_scan_root(Path::new("D:/音乐")).unwrap(); // 重复添加
        db.add_scan_root(Path::new("D:/其他")).unwrap();

        let roots = db.scan_roots().unwrap();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], "D:/音乐");

        db.remove_scan_root("D:/音乐").unwrap();
        assert_eq!(db.scan_roots().unwrap(), vec!["D:/其他".to_string()]);
    }

    #[test]
    fn sorted_by_artist_album_track() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        let mut b = meta("C:/m/b.mp3", 1, 1);
        b.artist = "B 乐队".into();
        b.album = "专辑".into();
        b.track_no = Some(1);
        let mut a2 = meta("C:/m/a2.mp3", 1, 1);
        a2.artist = "A 乐队".into();
        a2.album = "专辑".into();
        a2.track_no = Some(2);
        let mut a1 = meta("C:/m/a1.mp3", 1, 1);
        a1.artist = "A 乐队".into();
        a1.album = "专辑".into();
        a1.track_no = Some(1);

        db.upsert_track(&b, scan).unwrap();
        db.upsert_track(&a2, scan).unwrap();
        db.upsert_track(&a1, scan).unwrap();

        let all = db.all_tracks_sorted().unwrap();
        assert!(all[0].path.ends_with("a1.mp3"));
        assert!(all[1].path.ends_with("a2.mp3"));
        assert!(all[2].path.ends_with("b.mp3"));
    }

    #[test]
    fn track_by_id_finds_row_and_misses_cleanly() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 1, 1), scan).unwrap();
        let id = db.all_tracks_sorted().unwrap()[0].id;

        assert!(db.track_by_id(id).unwrap().is_some());
        assert!(db.track_by_id(id + 999).unwrap().is_none());
    }

    #[test]
    fn clear_tracks_empties_library() {
        let db = Db::open_in_memory().unwrap();
        let scan = db.begin_scan();
        db.upsert_track(&meta("C:/m/a.mp3", 1, 1), scan).unwrap();
        db.clear_tracks().unwrap();
        assert_eq!(db.track_count().unwrap(), 0);
    }
}
