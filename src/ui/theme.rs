//! 视觉主题。
//!
//! egui 的默认观感是「灰色方块 + 跟随系统主题」，用在音乐播放器上很糟：
//! 底色取决于系统（本机是刺眼的纯白），控件是 2px 直角，间距按桌面表单
//! 的密度来。这个模块把这些全部换掉。
//!
//! 三件事：
//! 1. **强制深色**，不跟随系统 —— 音乐播放器的封面在深色底上才好看；
//! 2. **配色**：接近纯黑但偏冷的底色 + 一个强调色，靠层次而不是线条分隔；
//! 3. **间距与圆角**：更大的行高与内边距，圆角化全部控件。

use eframe::egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Spacing, Stroke, Style, TextStyle,
    ThemePreference, Vec2, Visuals,
};

// ---------------------------------------------------------------------------
// 配色：冷调近黑 + 青色强调
// ---------------------------------------------------------------------------

/// 窗口最底层背景。
pub const BG: Color32 = Color32::from_rgb(0x0e, 0x10, 0x14);
/// 侧栏/卡片背景，比底色略亮一档，用来做层次。
pub const BG_PANEL: Color32 = Color32::from_rgb(0x16, 0x19, 0x1f);
/// 悬停态背景。
pub const BG_HOVER: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2d);
/// 选中/当前播放项的背景。
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x1b, 0x3a, 0x4d);
/// 分隔线。
pub const LINE: Color32 = Color32::from_rgb(0x25, 0x2a, 0x33);

/// 主要文字。
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xee);
/// 次要文字（艺术家、时长等）。
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xb2);
/// 更弱的文字（提示、占位）。
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x63, 0x6c, 0x7a);

/// 强调色。
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0xc4, 0xd6);
/// 强调色的低饱和版本，用作底。
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1d, 0x5a, 0x66);

/// 圆角尺寸。
const RADIUS: u8 = 8;

/// 曲库列表每一行的高度（不含行间距）。缩略图比它小一点，留出呼吸。
pub const ROW_HEIGHT: f32 = 56.0;
/// 列表缩略图边长。
pub const THUMB_SIZE: f32 = 40.0;

// ---------------------------------------------------------------------------
// 应用主题
// ---------------------------------------------------------------------------

/// 装上整套主题。在 `EchoApp::new` 里、字体之后调用。
pub fn apply(ctx: &eframe::egui::Context) {
    // 强制深色，不跟随系统 —— 本机系统是浅色，跟随的话底色会是一片白。
    ctx.set_theme(ThemePreference::Dark);

    let mut style = Style::default();

    style.visuals = dark_visuals();
    style.spacing = spacing();
    style.text_styles = text_styles();

    // 方法名是 set_global_style —— egui 0.36 里没有 set_style()。
    ctx.set_global_style(style);
}

/// 深色配色。
fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.panel_fill = BG;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x0a, 0x0c, 0x0f);
    v.faint_bg_color = BG_PANEL;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    // 「非交互」控件用于分隔线与缩略图占位。
    v.widgets.noninteractive.bg_fill = BG_PANEL;
    v.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    // 按钮的常态：比背景略亮，没有边框（靠层次而不是线框）。
    v.widgets.inactive.bg_fill = BG_PANEL;
    v.widgets.inactive.weak_bg_fill = BG_PANEL;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    v.widgets.inactive.corner_radius = CornerRadius::same(RADIUS);

    v.widgets.hovered.bg_fill = BG_HOVER;
    v.widgets.hovered.weak_bg_fill = BG_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.corner_radius = CornerRadius::same(RADIUS);

    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.corner_radius = CornerRadius::same(RADIUS);

    // 进度条/音量条：已播放部分用强调色填充（这个开关在 Visuals 上，
    // 不在 Spacing 里）。
    v.slider_trailing_fill = true;

    v
}

/// 间距节拍。默认值偏「桌面表单」的密度，这里放宽。
fn spacing() -> Spacing {
    let mut s = Spacing::default();

    s.item_spacing = Vec2::new(10.0, 8.0);
    s.button_padding = Vec2::new(12.0, 6.0);
    s.window_margin = Margin::same(14);
    s.menu_margin = Margin::same(8);
    s.indent = 18.0;
    s.interact_size = Vec2::new(40.0, 24.0);
    s.slider_width = 160.0;
    s.scroll = eframe::egui::style::ScrollStyle::solid();
    s.scroll.bar_width = 8.0;
    s.scroll.floating = false;

    s
}

/// 字号层次。标题更大，次要信息更小，拉开对比。
fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    use eframe::egui::TextStyle::*;
    [
        (Heading, FontId::new(24.0, FontFamily::Proportional)),
        (Body, FontId::new(14.5, FontFamily::Proportional)),
        (Monospace, FontId::new(13.5, FontFamily::Monospace)),
        (Button, FontId::new(14.5, FontFamily::Proportional)),
        (Small, FontId::new(12.5, FontFamily::Proportional)),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_not_a_greyscale() {
        // 强调色必须真的有色彩 —— 否则整套主题会退化成灰阶。
        assert!(
            ACCENT.r() != ACCENT.g() || ACCENT.g() != ACCENT.b(),
            "强调色应当是彩色的"
        );
    }

    #[test]
    fn background_is_dark() {
        // 深色主题的前提：底色亮度必须低。
        let luma = 0.299 * BG.r() as f32 + 0.587 * BG.g() as f32 + 0.114 * BG.b() as f32;
        assert!(luma < 60.0, "底色应当足够暗，实际亮度 {luma}");
    }

    #[test]
    fn text_contrasts_against_background() {
        let luma = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
        assert!(luma(TEXT) - luma(BG) > 120.0, "主要文字与底色对比应当明显");
        // 次要文字也要比底色亮，不能糊在一起。
        assert!(luma(TEXT_DIM) - luma(BG) > 60.0);
    }

    #[test]
    fn panel_is_lighter_than_background() {
        // 层次靠亮度差表达：面板要比底色亮一点。
        let luma = |c: Color32| c.r() as u16 + c.g() as u16 + c.b() as u16;
        assert!(luma(BG_PANEL) > luma(BG), "面板应当比底色亮");
        assert!(luma(BG_HOVER) > luma(BG_PANEL), "悬停应当比面板亮");
    }

    #[test]
    fn visuals_use_our_palette() {
        let v = dark_visuals();
        assert_eq!(v.panel_fill, BG);
        assert_eq!(v.hyperlink_color, ACCENT);
        // 圆角应当被应用，而不是默认的 2px 直角感。
        assert_eq!(v.widgets.inactive.corner_radius.nw, RADIUS);
    }

    #[test]
    fn spacing_is_roomier_than_default() {
        let custom = spacing();
        let default = Spacing::default();
        assert!(
            custom.item_spacing.y >= default.item_spacing.y,
            "间距不应比默认更挤"
        );
        assert!(custom.button_padding.x >= default.button_padding.x);
    }

    #[test]
    fn text_styles_have_hierarchy() {
        let styles = text_styles();
        let size = |k: &TextStyle| styles.get(k).map(|f| f.size).unwrap_or(0.0);
        assert!(size(&TextStyle::Heading) > size(&TextStyle::Body));
        assert!(size(&TextStyle::Body) > size(&TextStyle::Small));
    }
}
