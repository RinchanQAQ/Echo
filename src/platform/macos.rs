//! macOS 平台实现。
//!
//! 系统字体在 `/System/Library/Fonts` 与 `/Library/Fonts`。
//! 中文首选苹方（PingFang SC），它是 macOS 10.11 之后的中文界面字体。

use std::path::PathBuf;

use super::FontCandidate;

fn font_dirs() -> [PathBuf; 4] {
    [
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts/Supplemental"),
        PathBuf::from("/Library/Fonts"),
        PathBuf::from("/Network/Library/Fonts"),
    ]
}

pub fn cjk_font_candidates() -> Vec<FontCandidate> {
    let entries: &[(&str, &str)] = &[
        ("PingFang.ttc", "苹方"),
        // 冬青黑体：旧版 macOS 的中文界面字体，作为回退。
        ("Hiragino Sans GB.ttc", "冬青黑体简体中文"),
        ("STHeiti Light.ttc", "华文黑体"),
        ("STHeiti Medium.ttc", "华文黑体 Medium"),
        ("Songti.ttc", "宋体-简"),
    ];

    let dirs = font_dirs();
    let mut out = Vec::new();
    for (file, label) in entries {
        for dir in &dirs {
            out.push(FontCandidate {
                path: dir.join(file),
                label,
            });
        }
    }
    out
}

pub fn fallback_music_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Music")
}
