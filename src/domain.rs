//! 领域层：纯数据与纯逻辑。
//!
//! 这一层**刻意不依赖** egui / rodio / lofty / rusqlite / 任何平台 API，
//! 因此它可以在任何平台上编译与单测。它同时也是「换壳」的稳定契约：
//! 日后更换 GUI 框架或播放引擎，只需替换外层实现，这里不受影响。

// `PlayQueue` 提供的是完整的队列操作（next / prev / remove 等），
// 其中部分方法目前只被测试覆盖，保留以维持契约完整性。
#![allow(dead_code)]

use std::path::PathBuf;

/// 曲库中的一首曲目（对应 `tracks` 表的一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: i64,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u16>,
    pub genre: String,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub channels: Option<u8>,
    /// blake3 内容哈希，指向 `covers` 表；同一个封面在库中只存一份。
    pub cover_hash: Option<String>,
    pub file_size: u64,
    pub mtime: i64,
}

impl Track {
    /// 列表里展示的名字：优先标题，退化为文件名。
    pub fn display_title(&self) -> String {
        if self.title.is_empty() {
            self.path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
        } else {
            self.title.clone()
        }
    }

    /// 未知艺术家时的占位文案。
    pub fn display_artist(&self) -> &str {
        if self.artist.is_empty() {
            "未知艺术家"
        } else {
            &self.artist
        }
    }

    /// 格式化时长，例如 `3:07`。
    pub fn duration_text(&self) -> String {
        format_duration(std::time::Duration::from_millis(self.duration_ms))
    }
}

/// 把时长格式化为 `m:ss` 或 `h:mm:ss`。
pub fn format_duration(d: std::time::Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 扫描阶段读到的元数据（尚未落库，因此还没有 id）。
#[derive(Debug, Clone)]
pub struct TrackMeta {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u16>,
    pub genre: String,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub channels: Option<u8>,
    pub cover: Option<CoverArt>,
    pub file_size: u64,
    pub mtime: i64,
}

/// 刚从标签里读出的封面字节。落库时会按 `hash` 去重，之后 `bytes` 即可丢弃。
#[derive(Debug, Clone)]
pub struct CoverArt {
    pub hash: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// 扫描线程 → UI 的事件。UI 每帧用 `try_recv` 非阻塞消费，绝不阻塞渲染。
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// 已统计出待处理文件总数。
    Started {
        total: usize,
    },
    /// 已处理若干文件，`current` 用于展示当前文件名。
    Progress {
        done: usize,
        total: usize,
        current: String,
    },
    Finished(ScanSummary),
    /// 扫描整体失败（例如数据库打不开）。单文件错误计入 `ScanSummary::errors`。
    Failed(String),
}

/// 一次扫描的结果统计。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// 新入库的曲目数。
    pub added: usize,
    /// 已存在但元数据有变化的曲目数。
    pub updated: usize,
    /// 因文件未变而跳过的曲目数（快速路径命中）。
    pub skipped: usize,
    /// 从库中移除的曲目数（文件已消失）。
    pub removed: usize,
    /// 无法解析的文件数（损坏、编码不支持等）。
    pub errors: usize,
}

impl ScanSummary {
    /// 给状态栏用的一句话总结。
    pub fn describe(&self) -> String {
        let mut s = format!(
            "新增 {} 首，更新 {} 首，跳过 {} 首",
            self.added, self.updated, self.skipped
        );
        if self.removed > 0 {
            s.push_str(&format!("，移除 {} 首", self.removed));
        }
        if self.errors > 0 {
            s.push_str(&format!("，{} 个文件无法读取", self.errors));
        }
        s
    }
}

/// 循环模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// 播完最后一首停止。
    #[default]
    Off,
    /// 播完最后一首回到第一首。
    All,
    /// 单曲循环（自动切歌时原地重播）。
    One,
}

impl RepeatMode {
    /// 点击按钮时在三种模式间轮转。
    pub fn next_mode(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    /// 按钮上显示的图标。
    pub fn icon(self) -> &'static str {
        match self {
            Self::Off => "➡",
            Self::All => "🔁",
            Self::One => "🔂",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "顺序播放",
            Self::All => "列表循环",
            Self::One => "单曲循环",
        }
    }
}

/// 切歌原因，决定 `RepeatMode::One` 是否生效：
/// 自动播完时应重播本曲，而用户手动点「下一首」时应当真的换一首。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceKind {
    /// 当前曲目自然播放结束。
    Auto,
    /// 用户点击上一首/下一首。
    Manual,
}

/// 播放队列：曲目顺序 + 当前下标 + 循环模式。
///
/// 纯逻辑、无副作用，因此可以完整单测 —— 这正是把它放在领域层的原因。
#[derive(Debug, Clone, Default)]
pub struct PlayQueue {
    tracks: Vec<Track>,
    cursor: Option<usize>,
    repeat: RepeatMode,
}

impl PlayQueue {
    pub fn new(tracks: Vec<Track>) -> Self {
        let cursor = if tracks.is_empty() { None } else { Some(0) };
        Self {
            tracks,
            cursor,
            repeat: RepeatMode::Off,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = tracks;
        if self.tracks.is_empty() {
            self.cursor = None;
        } else if self.cursor.is_none_or(|c| c >= self.tracks.len()) {
            self.cursor = Some(0);
        }
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn current(&self) -> Option<&Track> {
        self.cursor.and_then(|i| self.tracks.get(i))
    }

    pub fn get(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// 直接跳到某个下标（点击列表行时用）。越界返回 `None` 且不改动状态。
    pub fn jump_to(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.cursor = Some(index);
            self.current()
        } else {
            None
        }
    }

    /// 计算下一首的下标，但不改动状态。
    pub fn peek_next(&self, kind: AdvanceKind) -> Option<usize> {
        self.advance(1, kind)
    }

    /// 计算上一首的下标，但不改动状态。
    pub fn peek_prev(&self, kind: AdvanceKind) -> Option<usize> {
        self.advance(-1, kind)
    }

    /// 切到下一首。返回新的当前曲目；若已到末尾且非循环，返回 `None` 并保持下标不变。
    pub fn next(&mut self, kind: AdvanceKind) -> Option<&Track> {
        if let Some(i) = self.peek_next(kind) {
            self.cursor = Some(i);
            self.current()
        } else {
            None
        }
    }

    /// 切到上一首。
    pub fn prev(&mut self, kind: AdvanceKind) -> Option<&Track> {
        if let Some(i) = self.peek_prev(kind) {
            self.cursor = Some(i);
            self.current()
        } else {
            None
        }
    }

    /// 从队列移除一曲（例如文件已删除），并修正下标。
    pub fn remove(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }
        self.tracks.remove(index);
        self.cursor = match self.cursor {
            None => None,
            Some(_) if self.tracks.is_empty() => None,
            // 被删的正是当前曲：原地顺延到下一首，末位则回退一首。
            Some(c) if c == index => Some(c.min(self.tracks.len() - 1)),
            // 被删的在当前曲之前：下标整体前移一位。
            Some(c) if c > index => Some(c - 1),
            Some(c) => Some(c),
        };
    }

    /// 从下标 `from` 出发按 `step` 前进/后退，统一处理循环、单曲与边界。
    fn advance(&self, step: isize, kind: AdvanceKind) -> Option<usize> {
        let cur = self.cursor?;
        if self.tracks.is_empty() {
            return None;
        }

        // 用户手动暂停在某一首上时，单曲循环不应妨碍他换歌。
        if self.repeat == RepeatMode::One && kind == AdvanceKind::Auto {
            return Some(cur);
        }

        let len = self.tracks.len() as isize;
        let next = cur as isize + step;

        match self.repeat {
            // 顺序播放：单向，到头即停。
            RepeatMode::Off => {
                if (0..len).contains(&next) {
                    Some(next as usize)
                } else {
                    None
                }
            }
            // 列表循环与单曲循环下手动切歌：两端环绕。
            RepeatMode::All | RepeatMode::One => Some(next.rem_euclid(len) as usize),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn track(n: usize) -> Track {
        Track {
            id: n as i64,
            path: PathBuf::from(format!("C:/music/{n}.mp3")),
            title: format!("曲目 {n}"),
            artist: "艺术家".into(),
            album: "专辑".into(),
            album_artist: String::new(),
            track_no: Some(n as u32),
            disc_no: None,
            year: Some(2024),
            genre: String::new(),
            duration_ms: 1000,
            sample_rate: Some(44100),
            bitrate: Some(320),
            channels: Some(2),
            cover_hash: None,
            file_size: 10,
            mtime: 0,
        }
    }

    fn queue(n: usize) -> PlayQueue {
        PlayQueue::new((0..n).map(track).collect())
    }

    #[test]
    fn empty_queue_has_no_cursor() {
        let q = PlayQueue::new(vec![]);
        assert!(q.is_empty());
        assert_eq!(q.cursor(), None);
        assert!(q.current().is_none());
    }

    #[test]
    fn non_empty_queue_starts_at_first() {
        let q = queue(3);
        assert_eq!(q.cursor(), Some(0));
        assert_eq!(q.current().unwrap().title, "曲目 0");
    }

    #[test]
    fn off_mode_stops_at_end() {
        let mut q = queue(3);
        q.set_repeat(RepeatMode::Off);
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 1");
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 2");
        // 末尾再前进应当停下，且下标保持不变。
        assert!(q.next(AdvanceKind::Manual).is_none());
        assert_eq!(q.cursor(), Some(2));
    }

    #[test]
    fn off_mode_stops_at_start_when_going_back() {
        let mut q = queue(3);
        assert!(q.prev(AdvanceKind::Manual).is_none());
        assert_eq!(q.cursor(), Some(0));
    }

    #[test]
    fn all_mode_wraps_both_ways() {
        let mut q = queue(3);
        q.set_repeat(RepeatMode::All);
        q.jump_to(2);
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 0");
        assert_eq!(q.prev(AdvanceKind::Manual).unwrap().title, "曲目 2");
    }

    #[test]
    fn one_mode_repeats_only_on_auto() {
        let mut q = queue(3);
        q.set_repeat(RepeatMode::One);
        // 自动播完：原地重播。
        assert_eq!(q.next(AdvanceKind::Auto).unwrap().title, "曲目 0");
        q.set_repeat(RepeatMode::All);
        // 手动切歌在 One 下也应换曲（此处用 All 验证对比）。
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 1");

        let mut q = queue(3);
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 1");
    }

    #[test]
    fn single_track_all_mode_stays_put() {
        let mut q = queue(1);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.next(AdvanceKind::Manual).unwrap().title, "曲目 0");
        assert_eq!(q.prev(AdvanceKind::Manual).unwrap().title, "曲目 0");
    }

    #[test]
    fn jump_out_of_range_is_ignored() {
        let mut q = queue(2);
        assert!(q.jump_to(5).is_none());
        assert_eq!(q.cursor(), Some(0));
        assert!(q.jump_to(1).is_some());
        assert_eq!(q.cursor(), Some(1));
    }

    #[test]
    fn remove_before_cursor_shifts_index() {
        let mut q = queue(4);
        q.jump_to(2);
        q.remove(0);
        assert_eq!(q.current().unwrap().title, "曲目 2");
        assert_eq!(q.cursor(), Some(1));
    }

    #[test]
    fn remove_current_keeps_position() {
        let mut q = queue(4);
        q.jump_to(1);
        q.remove(1);
        assert_eq!(q.current().unwrap().title, "曲目 2");
    }

    #[test]
    fn remove_last_current_falls_back() {
        let mut q = queue(3);
        q.jump_to(2);
        q.remove(2);
        assert_eq!(q.current().unwrap().title, "曲目 1");
        assert_eq!(q.cursor(), Some(1));
    }

    #[test]
    fn remove_all_clears_cursor() {
        let mut q = queue(1);
        q.remove(0);
        assert!(q.is_empty());
        assert_eq!(q.cursor(), None);
        assert!(q.current().is_none());
    }

    #[test]
    fn set_tracks_clamps_cursor() {
        let mut q = queue(5);
        q.jump_to(4);
        q.set_tracks((0..2).map(track).collect());
        assert_eq!(q.cursor(), Some(0));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn display_title_falls_back_to_filename() {
        let mut t = track(1);
        t.title = String::new();
        t.path = PathBuf::from("C:/music/我的歌.mp3");
        assert_eq!(t.display_title(), "我的歌");
        assert_eq!(t.display_artist(), "艺术家");

        t.artist = String::new();
        assert_eq!(t.display_artist(), "未知艺术家");
    }

    #[test]
    fn duration_formatting() {
        use std::time::Duration;
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(67)), "1:07");
        assert_eq!(format_duration(Duration::from_secs(3671)), "1:01:11");
    }

    #[test]
    fn repeat_mode_cycles() {
        assert_eq!(RepeatMode::Off.next_mode(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next_mode(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next_mode(), RepeatMode::Off);
    }

    #[test]
    fn summary_describes_errors_and_removals() {
        let s = ScanSummary {
            added: 1,
            updated: 2,
            skipped: 3,
            removed: 4,
            errors: 5,
        };
        let text = s.describe();
        assert!(text.contains("新增 1"));
        assert!(text.contains("移除 4"));
        assert!(text.contains("5 个文件无法读取"));
    }
}
