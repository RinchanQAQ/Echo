//! 界面复用组件。
//!
//! 全部与平台无关，只依赖 egui。

use egui::{Color32, TextureHandle, Ui, Vec2};

/// 列表缩略图的边长。
const THUMB_SIZE: f32 = 36.0;

/// 曲库列表里的一行。
///
/// 返回 `true` 表示这一行被点击了（调用方据此开始播放）。
/// 之所以返回布尔值而不是直接回调，是为了让调用方在同一帧内
/// 干净地结束对 `self` 的不可变借用 —— 播放需要 `&mut self`。
pub struct TrackRow<'a> {
    pub index: usize,
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: &'a str,
    /// 是否为当前正在播放的曲目。
    pub is_current: bool,
    /// 封面缩略图，无封面时画占位方块。
    pub thumbnail: Option<&'a TextureHandle>,
    /// 文件格式是否可播放（Opus 等只展示不播放）。
    pub playable: bool,
}

/// 画出一行曲目。返回是否被点击。
pub fn track_row(ui: &mut Ui, row: &TrackRow<'_>) -> bool {
    let mut clicked = false;

    ui.horizontal(|ui| {
        // ---- 缩略图 ----
        let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(THUMB_SIZE), egui::Sense::hover());
        match row.thumbnail {
            Some(tex) => {
                // 按短边等比缩放，避免宽高比失真的封面被拉变形。
                let size = tex.size_vec2();
                let scale = (THUMB_SIZE / size.x).max(THUMB_SIZE / size.y);
                let drawn = size * scale;
                let offset = (drawn - Vec2::splat(THUMB_SIZE)) * 0.5;
                let painter = ui.painter();
                painter.image(
                    tex.id(),
                    egui::Rect::from_min_size(rect.min - offset, drawn),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                // 无封面：画一个带边框的占位方块，让行高保持一致。
                let painter = ui.painter();
                painter.rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "♪",
                    egui::FontId::proportional(16.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }

        ui.add_space(6.0);

        // ---- 文字区 ----
        ui.vertical(|ui| {
            let mut title = row.title.to_owned();
            if row.is_current {
                title = format!("▶ {title}");
            }
            if !row.playable {
                title = format!("{title}（不可播放）");
            }
            ui.label(title);

            let mut sub = row.artist.to_owned();
            if !row.duration.is_empty() {
                sub = if sub.is_empty() {
                    row.duration.to_owned()
                } else {
                    format!("{} · {}", sub, row.duration)
                };
            }
            if !sub.is_empty() {
                ui.small(sub);
            }
        });

        // ---- 右侧留白，让整行都可点击 ----
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });

    // 用整行的矩形做一个覆盖式的点击感应区：
    // 这样点缩略图、点文字、点空白都能触发播放，而不是只有文字可点。
    let row_rect = ui.min_rect();
    let response = ui.interact(
        row_rect,
        ui.id().with(("track-row", row.index)),
        egui::Sense::click(),
    );
    if response.clicked() {
        clicked = true;
    }
    if response.hovered() {
        ui.painter().rect_filled(
            row_rect.expand(2.0),
            4.0,
            ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.35),
        );
    }

    clicked
}

/// 大封面展示区。返回实际占用的高度。
pub fn big_cover(ui: &mut Ui, handle: Option<&TextureHandle>, max_side: f32) -> f32 {
    match handle {
        Some(tex) => {
            let size = tex.size_vec2();
            if size.x <= 0.0 || size.y <= 0.0 {
                return placeholder_cover(ui, max_side);
            }
            let scale = (max_side / size.x).min(max_side / size.y);
            let drawn = size * scale;
            ui.add(egui::Image::new(tex).fit_to_exact_size(drawn));
            drawn.y
        }
        None => placeholder_cover(ui, max_side),
    }
}

fn placeholder_cover(ui: &mut Ui, side: f32) -> f32 {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
    painter.rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "无封面",
        egui::FontId::proportional(14.0),
        ui.visuals().weak_text_color(),
    );
    side
}

/// 顶部状态条：显示扫描进度与临时提示。
pub struct StatusBar<'a> {
    pub scanning: Option<&'a ScanProgressView>,
    pub message: Option<&'a str>,
    pub library_count: usize,
}

pub struct ScanProgressView {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

pub fn status_bar(ui: &mut Ui, bar: &StatusBar<'_>) {
    ui.horizontal(|ui| {
        if let Some(scan) = bar.scanning {
            let frac = if scan.total == 0 {
                0.0
            } else {
                scan.done as f32 / scan.total as f32
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(160.0)
                    .text(format!("扫描中 {}/{}", scan.done, scan.total)),
            );
            if !scan.current.is_empty() {
                ui.small(ellipsize(&scan.current, 60));
            }
        } else if let Some(msg) = bar.message {
            ui.label(msg);
        } else {
            ui.small(format!("共 {} 首曲目", bar.library_count));
        }
    });
}

/// 把过长的路径截断成「前面…后面」的形式，避免状态栏被撑爆。
pub fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(count - keep).collect();
    format!("{head}…{tail}")
}

/// 把秒数格式化为进度条用的 Duration。
pub fn secs_to_duration(secs: f64) -> std::time::Duration {
    std::time::Duration::from_secs_f64(secs.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsize_keeps_short_strings_intact() {
        assert_eq!(ellipsize("短", 10), "短");
        assert_eq!(ellipsize("abc", 3), "abc");
    }

    #[test]
    fn ellipsize_truncates_long_strings() {
        let long = "这是一个非常长的文件路径/专辑名/曲目名称.mp3";
        let out = ellipsize(long, 12);
        assert!(out.chars().count() <= 12, "结果应当不超过上限：{out}");
        assert!(out.contains('…'));
    }

    #[test]
    fn ellipsize_is_utf8_safe() {
        // 按字符切分而不是按字节，中文字符不应被切碎。
        let s = "中文测试字符串";
        let out = ellipsize(s, 5);
        assert!(out.chars().count() <= 5);
        // 能正常显示说明没有产生非法 UTF-8。
        assert!(out.chars().all(|c| c == '…' || s.contains(c)));
    }

    #[test]
    fn secs_to_duration_clamps_negative() {
        assert_eq!(secs_to_duration(-5.0), std::time::Duration::ZERO);
        assert_eq!(secs_to_duration(1.5).as_millis(), 1500);
    }
}
