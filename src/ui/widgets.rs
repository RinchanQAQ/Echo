//! 界面复用组件。
//!
//! 全部与平台无关，只依赖 egui。

use egui::{Color32, CornerRadius, TextureHandle, Ui, Vec2};

use super::theme;

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

/// 画出一行曲目，高度固定为 [`theme::ROW_HEIGHT`]。返回是否被点击。
///
/// 高度固定是**虚拟化列表的前提**：`ScrollArea::show_rows` 需要知道行高
/// 才能只渲染可见区间。之前这里是自适应高度，因此只能全量渲染，
/// 几千首曲目会明显卡顿。
///
/// 关键点：必须用 `allocate_exact_size` 真正**占用**这段高度。
/// 只把内容画进一个 `max_rect` 子 Ui 是不够的 —— egui 会按内容自动收缩，
/// 游标不前进，于是每一行都画在同一个位置，视觉上叠成一团
/// （这个 bug 实际发生过）。
pub fn track_row(ui: &mut Ui, row: &TrackRow<'_>) -> bool {
    let full_width = ui.available_width();

    // 真正占用空间：让游标前进整整一行，虚拟化才能对齐。
    let (row_rect, response) = ui.allocate_exact_size(
        Vec2::new(full_width, theme::ROW_HEIGHT),
        egui::Sense::click(),
    );

    // ---- 背景（先画，避免盖住文字）----
    let radius = CornerRadius::same(8);
    if row.is_current {
        ui.painter()
            .rect_filled(row_rect, radius, theme::ACCENT_DIM);
        // 左侧竖条，让「正在播放」更醒目。
        let bar = egui::Rect::from_min_size(
            row_rect.min + Vec2::new(0.0, 8.0),
            Vec2::new(3.0, row_rect.height() - 16.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
    } else if response.hovered() {
        ui.painter().rect_filled(row_rect, radius, theme::BG_HOVER);
    }

    // ---- 内容 ----
    let content_rect = row_rect.shrink2(Vec2::new(10.0, 8.0));
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    // 缩略图：圆角 + 等比裁剪。
    let (thumb_rect, _) =
        content.allocate_exact_size(Vec2::splat(theme::THUMB_SIZE), egui::Sense::hover());
    let thumb_radius = CornerRadius::same(6);
    match row.thumbnail {
        Some(tex) => {
            let painter = content.painter();
            painter.rect_filled(thumb_rect, thumb_radius, theme::BG_HOVER);
            // 裁剪到圆角矩形内，避免非方形封面溢出。
            let clipped = painter.with_clip_rect(thumb_rect);
            let size = tex.size_vec2();
            if size.x > 0.0 && size.y > 0.0 {
                let scale = (theme::THUMB_SIZE / size.x).max(theme::THUMB_SIZE / size.y);
                let drawn = size * scale;
                let offset = (drawn - Vec2::splat(theme::THUMB_SIZE)) * 0.5;
                clipped.image(
                    tex.id(),
                    egui::Rect::from_min_size(thumb_rect.min - offset, drawn),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        None => {
            let painter = content.painter();
            painter.rect_filled(thumb_rect, thumb_radius, theme::BG_HOVER);
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "♪",
                egui::FontId::proportional(16.0),
                theme::TEXT_FAINT,
            );
        }
    }

    content.add_space(12.0);

    // 右侧时长先占位，剩下的宽度给标题/艺术家。
    let duration_width = 52.0;
    let text_width = (content.available_width() - duration_width).max(40.0);

    content.allocate_ui_with_layout(
        Vec2::new(text_width, content_rect.height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;

            let title = if row.is_current {
                egui::RichText::new(format!("▶ {}", row.title)).color(theme::ACCENT)
            } else {
                egui::RichText::new(row.title).color(theme::TEXT)
            };
            let title = if row.playable { title } else { title.weak() };
            // 标题过长时截断，避免把时长挤出去。
            ui.add(egui::Label::new(title).truncate());

            let mut sub = row.artist.to_owned();
            if !row.playable {
                sub = if sub.is_empty() {
                    "不可播放".to_owned()
                } else {
                    format!("{sub} · 不可播放")
                };
            }
            if !sub.is_empty() {
                ui.add(
                    egui::Label::new(egui::RichText::new(sub).small().color(theme::TEXT_DIM))
                        .truncate(),
                );
            }
        },
    );

    // 时长靠右。
    content.allocate_ui_with_layout(
        Vec2::new(duration_width, content_rect.height()),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if !row.duration.is_empty() {
                ui.label(
                    egui::RichText::new(row.duration)
                        .small()
                        .color(theme::TEXT_FAINT),
                );
            }
        },
    );

    response.clicked()
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
