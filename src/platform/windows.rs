//! Windows 平台实现。
//!
//! 系统字体放在 `%WINDIR%\Fonts`。这里优先挑「界面用」的现代无衬线字体：
//! 微软雅黑（msyh）字形覆盖最全、观感最统一；其次才是黑体/等线/宋体兜底。
//!
//! 注意 `.ttc` 是字体集合（一个文件里含多个字重），egui 能接受，
//! 但我们在排序上把 `.ttf` 放在前面，因为单一字形的文件解析失败风险更低。

use std::path::PathBuf;

use super::FontCandidate;

/// 系统字体目录。理论上能用真实 `GetWindowsDirectory` 拿到，
/// 但 `%WINDIR%` 环境变量在所有正常安装的 Windows 上都有，且无额外依赖。
fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("Fonts")
}

pub fn cjk_font_candidates() -> Vec<FontCandidate> {
    let dir = fonts_dir();
    let entries: &[(&str, &str)] = &[
        // 微软雅黑：简体中文界面首选。
        ("msyh.ttc", "微软雅黑"),
        ("msyh.ttf", "微软雅黑"),
        ("msyhl.ttc", "微软雅黑 Light"),
        // 黑体。
        ("simhei.ttf", "黑体"),
        // 等线：较新的 Windows 默认中文字体。
        ("Deng.ttf", "等线"),
        ("Dengl.ttf", "等线 Light"),
        // 正黑体系（繁体覆盖更好）。
        ("msjh.ttc", "微软正黑"),
        // 宋体兜底。
        ("simsun.ttc", "宋体"),
        ("simsunb.ttf", "新宋体"),
    ];

    entries
        .iter()
        .map(|(file, label)| FontCandidate {
            path: dir.join(file),
            label,
        })
        .collect()
}

/// 用户目录下的「音乐」文件夹（英文名与中文名都试）。
pub fn fallback_music_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
        .join("Music")
}
