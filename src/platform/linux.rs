//! Linux 平台实现。
//!
//! 系统字体分散在若干标准目录里（XDG 规范 + 发行版约定），
//! 因此这里先给出确定的路径，再用目录扫描兜底。
//!
//! 刻意**不**依赖 fontconfig：Cargo.toml 里对非 Windows/macOS 平台
//! 关闭了 `fontique` 的 `system` 特性，这样在 Linux 上编译无需
//! `libfontconfig-dev`，对用户更友好。

use std::path::PathBuf;

use super::{FontCandidate, scan_dirs_for_fonts};

/// 常见的系统字体根目录。
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
        PathBuf::from("/usr/share/fonts/truetype"),
        PathBuf::from("/usr/share/fonts/opentype"),
        // 部分发行版把 Noto CJK 放在这里。
        PathBuf::from("/usr/share/fonts/noto-cjk"),
        PathBuf::from("/usr/share/fonts/google-noto-cjk"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".fonts"));
        dirs.push(home.join(".local/share/fonts"));
    }
    dirs
}

/// 按文件名关键词扫描字体目录，顺序即优先级。
fn scan(needles: &[&str]) -> Vec<FontCandidate> {
    // 深度 4 足以覆盖 /usr/share/fonts/<vendor>/<family>/<file> 这类布局。
    scan_dirs_for_fonts(&font_dirs(), needles, 4)
}

pub fn cjk_font_candidates() -> Vec<FontCandidate> {
    let mut out = Vec::new();

    // 第一优先：确定性路径（命中时最快，也最可预期）。
    for (path, label) in [
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK",
        ),
        (
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK",
        ),
        (
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "Noto Serif CJK",
        ),
        ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", "文泉驿正黑"),
        (
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "文泉驿微米黑",
        ),
    ] {
        out.push(FontCandidate {
            path: PathBuf::from(path),
            label,
        });
    }

    // 第二优先：扫描。发行版布局千差万别，扫描能覆盖绝大多数情况。
    out.extend(scan(&[
        "NotoSansCJK",
        "NotoSerifCJK",
        "SourceHanSans",
        "SourceHanSerif",
        "wqy-zenhei",
        "wqy-microhei",
        "DroidSansFallback",
        "arphic",
        "uming",
        "ukai",
    ]));

    out
}

pub fn fallback_music_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Music")
}
