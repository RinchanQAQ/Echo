//! 扫描层：后台遍历目录并把曲目写入数据库。
//!
//! 这一层完全跨平台。设计要点：
//! - 扫描在**独立线程**里跑，UI 线程绝不参与文件 I/O；
//! - 进度通过 channel 上报，UI 每帧 `try_recv` 非阻塞消费；
//! - 「未变更则跳过」用 (文件大小, 修改时间) 判定，命中时连标签都不解析；
//! - 标签解析是 I/O 密集型，用 rayon 并行；**数据库写入串行**，避免锁竞争。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::db::Db;
use crate::domain::{ScanEvent, ScanSummary, TrackMeta};
use crate::metadata::{file_signature, is_audio_path, read_track};

/// 启动一次扫描，立刻返回接收端。
///
/// `roots` 是要扫描的目录列表；`db` 会在后台线程里使用（内部是 Arc<Mutex>）。
pub fn start_scan(roots: Vec<PathBuf>, db: Db) -> Receiver<ScanEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    thread::Builder::new()
        .name("echo-scan".to_string())
        .spawn(move || {
            let send = |ev: ScanEvent| {
                // 接收端被丢弃（例如窗口已关闭）时静默退出，不要 panic。
                let _ = tx.send(ev);
            };
            match run_scan(&roots, &db, &send) {
                Ok(summary) => send(ScanEvent::Finished(summary)),
                Err(e) => send(ScanEvent::Failed(format!("{e:#}"))),
            }
        })
        .expect("无法创建扫描线程");
    rx
}

/// 在调用线程上执行扫描（测试直接调用它，避免线程时序问题）。
pub fn run_scan(
    roots: &[PathBuf],
    db: &Db,
    send: &dyn Fn(ScanEvent),
) -> anyhow::Result<ScanSummary> {
    let scan_id = db.begin_scan();

    // ---- 第一遍：遍历目录，收集候选文件及其当前签名 ----
    let mut candidates: Vec<(PathBuf, u64, i64)> = Vec::new();
    for root in roots {
        if !root.exists() {
            log::warn!("扫描目录不存在，已跳过：{}", root.display());
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| match e {
                Ok(e) => Some(e),
                Err(err) => {
                    // 权限不足的目录等：跳过而不是中断整次扫描。
                    log::debug!("遍历跳过：{err}");
                    None
                }
            })
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !is_audio_path(path) {
                continue;
            }
            match file_signature(path) {
                Ok((size, mtime)) => candidates.push((path.to_path_buf(), size, mtime)),
                Err(e) => log::debug!("无法读取文件属性，跳过 {}：{e}", path.display()),
            }
        }
    }

    let total = candidates.len();
    send(ScanEvent::Started { total });

    // ---- 第二遍：区分「已变更」与「未变更」 ----
    let mut to_parse: Vec<(PathBuf, u64, i64)> = Vec::with_capacity(total);
    // 走快速路径跳过的文件，稍后要统一刷新它们的 last_seen_scan。
    let mut unchanged: Vec<String> = Vec::new();
    let mut summary = ScanSummary::default();
    let mut done = 0usize;

    for (path, size, mtime) in candidates {
        let key = path.to_string_lossy().into_owned();
        match db.existing_signature(&key) {
            // 快速路径：文件没变过，完全不必解析标签。
            //
            // 两边都统一成 u64 再比：数据库里的 file_size 是 INTEGER，
            // 但经过行映射后是 u64（负数会被夹到 0），而候选里的 size
            // 来自 fs::metadata().len() 也是 u64。
            Ok(Some(((db_size, db_mtime), _))) if db_size == size && db_mtime == mtime => {
                summary.skipped += 1;
                done += 1;
                unchanged.push(key);
                continue;
            }
            // 新文件或已变更，需要解析。
            _ => to_parse.push((path, size, mtime)),
        }
    }

    if !to_parse.is_empty() {
        send(ScanEvent::Progress {
            done,
            total,
            current: format!("正在读取 {} 个文件的标签…", to_parse.len()),
        });
    }

    // ---- 第三遍：并行解析标签，串行写库 ----
    // 单独建一个线程池，避免影响进程内其它 rayon 使用者。
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(default_parse_threads())
        .thread_name(|i| format!("echo-parse-{i}"))
        .build()?;

    // 分块是为了在解析与写库之间取得平衡：块太大则进度更新迟钝，
    // 太小则线程调度开销上升。
    const CHUNK: usize = 64;

    for chunk in to_parse.chunks(CHUNK) {
        let parsed: Vec<ParsedFile> = pool.install(|| {
            chunk
                .par_iter()
                .map(|(path, size, mtime)| parse_one(path, *size, *mtime))
                .collect()
        });

        for item in parsed {
            done += 1;
            send(ScanEvent::Progress {
                done,
                total,
                current: item
                    .meta
                    .as_ref()
                    .map(|m| m.path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| item.path.to_string_lossy().into_owned()),
            });

            match item.meta {
                Some(meta) => {
                    // 先落封面再写曲目，避免 cover_hash 指向不存在的行。
                    if let Some(cover) = &meta.cover
                        && let Err(e) = db.put_cover(cover)
                    {
                        log::warn!("写入封面失败：{e}");
                    }
                    match db.upsert_track(&meta, scan_id) {
                        Ok(true) => summary.added += 1,
                        Ok(false) => summary.updated += 1,
                        Err(e) => {
                            summary.errors += 1;
                            log::warn!("写入曲目失败 {}：{e}", meta.path.display());
                        }
                    }
                }
                None => {
                    // 解析失败但文件还在：写一条仅含大小/时间的占位记录，
                    // 这样它不会被「本次未见到的行」清理逻辑误删。
                    summary.errors += 1;
                    if let Err(e) = db.upsert_stub(&item.path, item.size, item.mtime, scan_id) {
                        log::warn!("写入占位记录失败 {}：{e}", item.path.display());
                    }
                }
            }
        }
    }

    // ---- 收尾：刷新跳过项、清理已消失的文件与孤儿封面 ----
    // 必须先 touch 再 delete：跳过解析的文件不会走 upsert，
    // 若不刷新 last_seen_scan，它们会被下面的清理逻辑当成陈旧数据删掉。
    db.touch_tracks(&unchanged, scan_id)?;
    summary.removed = db.delete_tracks_not_in_scan(scan_id)?;
    if let Err(e) = db.prune_orphan_covers() {
        log::warn!("清理孤儿封面失败：{e}");
    }

    Ok(summary)
}

struct ParsedFile {
    path: PathBuf,
    size: u64,
    mtime: i64,
    /// 解析成功时的完整元数据；失败则为 `None`。
    meta: Option<TrackMeta>,
}

fn parse_one(path: &Path, size: u64, mtime: i64) -> ParsedFile {
    match read_track(path) {
        Ok(meta) => ParsedFile {
            path: path.to_path_buf(),
            size,
            mtime,
            meta: Some(meta),
        },
        Err(e) => {
            log::debug!("解析失败 {}：{e:#}", path.display());
            ParsedFile {
                path: path.to_path_buf(),
                size,
                mtime,
                meta: None,
            }
        }
    }
}

/// 解析线程数：留一个核给 UI 线程，避免扫描时界面掉帧。
fn default_parse_threads() -> usize {
    let n = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    n.saturating_sub(1).max(1)
}

/// 把事件转发到 channel，忽略接收端已关闭的情况。
pub fn send_event(tx: &Sender<ScanEvent>, ev: ScanEvent) {
    let _ = tx.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::write_test_wav;

    /// 同步收集事件的测试辅助。
    ///
    /// 不能用「后台线程 + channel」的写法：`run_scan` 在当前线程同步执行，
    /// 它返回时收集线程可能还没把事件排空，断言就会看到空列表。
    ///
    /// 用 `RefCell` 而不是 `&mut`：`run_scan` 要的回调是 `Fn`（可被多次调用、
    /// 不可变借用），所以需要在回调内部用内部可变性来记录。
    #[derive(Default)]
    struct Recorder {
        events: std::cell::RefCell<Vec<ScanEvent>>,
    }

    impl Recorder {
        fn run(&self, roots: &[PathBuf], db: &Db) -> anyhow::Result<ScanSummary> {
            run_scan(roots, db, &|ev| self.events.borrow_mut().push(ev))
        }

        fn has_started_with(&self, total: usize) -> bool {
            self.events
                .borrow()
                .iter()
                .any(|e| matches!(e, ScanEvent::Started { total: t } if *t == total))
        }

        fn saw_progress(&self) -> bool {
            self.events
                .borrow()
                .iter()
                .any(|e| matches!(e, ScanEvent::Progress { .. }))
        }

        fn debug_events(&self) -> String {
            format!("{:?}", self.events.borrow())
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_audio_and_skips_non_audio() {
        let dir = temp_dir("basic");
        write_test_wav(&dir.join("a.wav"), 1).unwrap();
        write_test_wav(&dir.join("b.wav"), 1).unwrap();
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.join("cover.jpg"), b"not audio").unwrap();

        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        let summary = rec.run(std::slice::from_ref(&dir), &db).unwrap();

        assert_eq!(summary.added, 2, "应当只入库两个 wav");
        assert_eq!(db.track_count().unwrap(), 2);

        // 应当收到 Started(total=2，非音频文件不计入) 与至少一条 Progress。
        assert!(
            rec.has_started_with(2),
            "应当上报总数为 2 的 Started 事件，实际: {}",
            rec.debug_events()
        );
        assert!(rec.saw_progress(), "应当上报进度");
    }

    #[test]
    fn rescan_uses_fast_path_and_does_not_duplicate() {
        let dir = temp_dir("rescan");
        write_test_wav(&dir.join("a.wav"), 1).unwrap();

        let db = Db::open_in_memory().unwrap();

        let first = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();
        assert_eq!(first.added, 1);
        assert_eq!(first.skipped, 0);

        // 再扫一次：文件未变，应当走快速路径跳过，且不新增行。
        let second = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.skipped, 1, "未变更的文件应当被跳过");
        assert_eq!(second.removed, 0);
        assert_eq!(db.track_count().unwrap(), 1, "不得重复入库");
    }

    #[test]
    fn changed_file_is_reparsed() {
        let dir = temp_dir("changed");
        let wav = dir.join("a.wav");
        write_test_wav(&wav, 1).unwrap();
        let db = Db::open_in_memory().unwrap();
        run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();

        // 改写文件内容（大小变化必然导致签名变化）。
        write_test_wav(&wav, 2).unwrap();
        let second = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();

        assert_eq!(second.skipped, 0);
        assert_eq!(second.updated, 1);
        assert_eq!(db.track_count().unwrap(), 1);
    }

    #[test]
    fn deleted_file_is_removed_from_library() {
        let dir = temp_dir("deleted");
        let keep = dir.join("keep.wav");
        let gone = dir.join("gone.wav");
        write_test_wav(&keep, 1).unwrap();
        write_test_wav(&gone, 1).unwrap();

        let db = Db::open_in_memory().unwrap();
        assert_eq!(
            run_scan(std::slice::from_ref(&dir), &db, &|_| {})
                .unwrap()
                .added,
            2
        );

        std::fs::remove_file(&gone).unwrap();
        let second = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();
        assert_eq!(second.removed, 1);
        assert_eq!(db.track_count().unwrap(), 1);
    }

    #[test]
    fn unreadable_audio_is_counted_but_not_lost() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join("broken.mp3"), "这不是真的 mp3".as_bytes()).unwrap();
        write_test_wav(&dir.join("good.wav"), 1).unwrap();

        let db = Db::open_in_memory().unwrap();
        let summary = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();

        assert_eq!(summary.added, 1);
        assert!(summary.errors >= 1, "损坏文件应当计入错误数");
        // 占位记录也要在库里，否则下次扫描会被当成「已消失」而删除。
        assert_eq!(
            db.track_count().unwrap(),
            2,
            "损坏文件应当留下占位记录：{summary:?}"
        );
    }

    #[test]
    fn missing_root_does_not_fail_scan() {
        let dir = temp_dir("missingroot");
        let db = Db::open_in_memory().unwrap();
        let bogus = dir.join("不存在的目录");
        let summary = run_scan(&[bogus], &db, &|_| {}).unwrap();
        assert_eq!(summary.added, 0);
        assert_eq!(db.track_count().unwrap(), 0);
    }

    #[test]
    fn nested_directories_are_walked() {
        let dir = temp_dir("nested");
        let sub = dir.join("专辑").join("disc1");
        std::fs::create_dir_all(&sub).unwrap();
        write_test_wav(&sub.join("deep.wav"), 1).unwrap();

        let db = Db::open_in_memory().unwrap();
        let summary = run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();
        assert_eq!(summary.added, 1);
    }

    #[test]
    fn start_scan_reports_finished_over_channel() {
        let dir = temp_dir("channel");
        write_test_wav(&dir.join("a.wav"), 1).unwrap();
        let db = Db::open_in_memory().unwrap();
        let rx = start_scan(vec![dir], db);

        let mut saw_started = false;
        let mut summary = None;
        while let Ok(ev) = rx.recv() {
            match ev {
                ScanEvent::Started { .. } => saw_started = true,
                ScanEvent::Finished(s) => {
                    summary = Some(s);
                    break;
                }
                ScanEvent::Failed(e) => panic!("扫描失败: {e}"),
                ScanEvent::Progress { .. } => {}
            }
        }
        assert!(saw_started);
        assert_eq!(summary.unwrap().added, 1);
    }
}
