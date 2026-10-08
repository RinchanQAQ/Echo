//! 平台层：**整个项目里唯一出现操作系统差异的地方**。
//!
//! 设计约定：
//! - 上层模块（db / metadata / library / playback / ui）一律不做平台判定；
//! - 新增一个平台支持，只需在这里加一个子模块并在 `mod` 里挂上；
//! - 所有平台特有逻辑都必须能在无该平台的情况下被编译掉（`#[cfg]`）。
//!
//! 关于字体选择的实现说明：这里采用「按候选路径读取字体文件」而不是
//! 「用 fontique 枚举系统字体」。原因是三平台下按路径读取的代码完全一致、
//! 易于单测，并且避免了在 Linux 上引入 fontconfig 依赖。

// 平台层对外提供的是一组完整能力（默认音乐目录、目录扫描等），
// 其中部分目前在界面上还没有入口，保留以便后续功能使用。
#![allow(dead_code)]

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// 应用需要知道的路径。
#[derive(Debug, Clone)]
pub struct PlatformPaths {
    /// 存放 library.db 等数据的目录。
    pub data_dir: PathBuf,
}

impl PlatformPaths {
    /// 数据库文件位置。
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("library.db")
    }
}

/// 取本平台的应用数据目录。
///
/// 用 `directories` 而非手写路径，三平台差异由它处理：
/// - Windows: `%APPDATA%\echo\Echo\data`
/// - macOS:   `~/Library/Application Support/dev.echo.Echo`
/// - Linux:   `~/.local/share/echo`
pub fn paths() -> PlatformPaths {
    let data_dir = directories::ProjectDirs::from("dev", "echo", "Echo")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    PlatformPaths { data_dir }
}

/// 一个候选字体文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontCandidate {
    pub path: PathBuf,
    /// 人类可读的名字，仅用于日志。
    pub label: &'static str,
}

/// 返回本平台上按优先级排列的中文字体候选路径。
///
/// 顺序原则：优先「界面用」的现代无衬线字体（观感好、字重全），
/// 其次才是衬线/黑体兜底。返回的列表里可能包含不存在的路径，
/// 调用方需要逐个尝试。
pub fn cjk_font_candidates() -> Vec<FontCandidate> {
    imp::cjk_font_candidates()
}

/// 默认要提示用户添加的音乐目录（可能不存在，仅用于首次启动时的建议）。
pub fn default_music_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dirs) = directories::UserDirs::new() {
        if let Some(audio) = dirs.audio_dir() {
            out.push(audio.to_path_buf());
        }
        out.push(dirs.home_dir().join("Music"));
    }
    out.push(imp::fallback_music_dir());
    out.retain(|p| p.exists() || p.parent().is_some_and(|q| q.exists()));
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// 跨平台的字体解析
// ---------------------------------------------------------------------------

/// 一个可交给 egui 使用的字体文件内容。
#[derive(Debug, Clone)]
pub struct LoadedFont {
    /// 字体文件字节（可能是 .ttf 或 .ttc）。
    pub bytes: Vec<u8>,
    /// 来源路径，用于日志。
    pub path: PathBuf,
    /// 展示名。
    pub label: &'static str,
}

/// 按候选列表逐个尝试，返回第一个能读出来且看起来是字体的文件。
///
/// 这里的「看起来是字体」判定是读前 4 个字节的 sfnt 魔数：
/// `0x00010000`（TrueType）、`OTTO`（CFF）、`true`、`ttcf`（字体集合）。
/// 有些系统字体文件其实是损坏的或只是个占位符，提前挡掉可以避免
/// egui 在解析阶段 panic 或静默丢字。
pub fn load_first_available_font(candidates: &[FontCandidate]) -> Option<LoadedFont> {
    for cand in candidates {
        let Ok(bytes) = std::fs::read(&cand.path) else {
            continue;
        };
        if !looks_like_font(&bytes) {
            log::debug!("跳过非字体文件：{}", cand.path.display());
            continue;
        }
        log::info!("已加载中文字体：{}（{}）", cand.label, cand.path.display());
        return Some(LoadedFont {
            bytes,
            path: cand.path.clone(),
            label: cand.label,
        });
    }
    log::warn!("未找到任何可用的中文字体，界面中的中文可能显示为方块。");
    None
}

/// 判断字节是否像一个 sfnt 字体文件。
pub fn looks_like_font(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    matches!(
        &bytes[0..4],
        [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true" | b"ttcf"
    )
}

/// 从候选目录里按文件名关键词找字体（Linux 用得多）。
///
/// `needles` 是文件名里应包含的关键词（大小写不敏感）。
/// 结果按 `needles` 的顺序返回，同组内按文件名排序保证可复现。
#[allow(dead_code)] // 仅部分平台使用
pub fn scan_dirs_for_fonts(
    dirs: &[PathBuf],
    needles: &[&str],
    max_depth: usize,
) -> Vec<FontCandidate> {
    let mut found: Vec<FontCandidate> = Vec::new();
    for needle in needles {
        let needle_lower = needle.to_ascii_lowercase();
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            let walker = walkdir::WalkDir::new(dir)
                .max_depth(max_depth)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok());
            let mut hits: Vec<PathBuf> = Vec::new();
            for entry in walker {
                if !entry.file_type().is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if !(name.ends_with(".ttf") || name.ends_with(".ttc") || name.ends_with(".otf")) {
                    continue;
                }
                if name.contains(&needle_lower) {
                    hits.push(entry.path().to_path_buf());
                }
            }
            hits.sort();
            for path in hits {
                found.push(FontCandidate {
                    path,
                    label: "系统字体",
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_valid_font_magics() {
        assert!(looks_like_font(&[0x00, 0x01, 0x00, 0x00, 0, 0]));
        assert!(looks_like_font(b"OTTO____"));
        assert!(looks_like_font(b"true____"));
        assert!(looks_like_font(b"ttcf____"));
    }

    #[test]
    fn rejects_non_font_data() {
        assert!(!looks_like_font(b""));
        assert!(!looks_like_font(b"ab"));
        assert!(!looks_like_font(b"PNG\x89"));
        assert!(!looks_like_font("这不是字体".as_bytes()));
    }

    #[test]
    fn db_path_sits_under_data_dir() {
        let p = PlatformPaths {
            data_dir: PathBuf::from("C:/data/echo"),
        };
        assert!(p.db_path().ends_with("library.db"));
        assert!(p.db_path().starts_with("C:/data/echo"));
    }

    #[test]
    fn candidate_list_is_not_empty_on_this_platform() {
        // 三平台都应当至少给出一个候选（即使文件不存在）。
        assert!(
            !cjk_font_candidates().is_empty(),
            "每个平台都应当提供中文字体候选"
        );
    }

    #[test]
    fn missing_candidates_are_skipped_without_panic() {
        let bogus = vec![
            FontCandidate {
                path: PathBuf::from("C:/绝对不存在的字体.ttf"),
                label: "不存在",
            },
            FontCandidate {
                path: PathBuf::from("C:/也没有这个.ttc"),
                label: "不存在",
            },
        ];
        assert!(load_first_available_font(&bogus).is_none());
    }

    #[test]
    fn non_font_file_is_rejected() {
        let dir = std::env::temp_dir().join(format!("echo-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fake.ttf");
        std::fs::write(&fake, "这其实是个文本文件".as_bytes()).unwrap();

        let cands = vec![FontCandidate {
            path: fake,
            label: "假字体",
        }];
        assert!(
            load_first_available_font(&cands).is_none(),
            "魔数不符的文件不应被当成字体"
        );
    }

    #[test]
    fn scan_finds_nothing_in_empty_dir() {
        let dir = std::env::temp_dir().join(format!("echo-fontscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let found = scan_dirs_for_fonts(&[dir], &["NotoSansCJK"], 4);
        assert!(found.is_empty());
    }

    #[test]
    fn scan_respects_needle_order() {
        let dir = std::env::temp_dir().join(format!("echo-fontscan2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wqy-zenhei.ttc"), b"ttcf").unwrap();
        std::fs::write(dir.join("NotoSansCJK-Regular.ttc"), b"ttcf").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();

        // 关键词顺序决定优先级：NotoSansCJK 应当排在 wqy 之前。
        let found = scan_dirs_for_fonts(&[dir], &["NotoSansCJK", "wqy"], 2);
        assert_eq!(found.len(), 2);
        assert!(found[0].path.to_string_lossy().contains("NotoSansCJK"));
        assert!(found[1].path.to_string_lossy().contains("wqy"));
    }

    #[test]
    fn default_music_dirs_do_not_panic() {
        // 只验证不 panic 且结果去重。
        let dirs = default_music_dirs();
        let mut sorted = dirs.clone();
        sorted.dedup();
        assert_eq!(dirs.len(), sorted.len());
    }
}
