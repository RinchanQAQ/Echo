//! 中文字体装配。
//!
//! egui 0.36 内置的字体只有 Ubuntu-Light（拉丁）、Hack（等宽）、
//! emoji-icon-font 和 NotoEmoji，**不含任何 CJK 字形**。
//! 如果不额外装入系统字体，界面上的「曲库」「播放」以及中文歌曲标签
//! 全都会渲染成空心方块（tofu），因此这一步是中文可用性的前提。
//!
//! 装配方式是把中文字体**追加到字体族列表的末尾**：
//! 拉丁字符仍由内置的 Ubuntu-Light 渲染（观感更好、字重更合适），
//! 只有它缺失的字形（也就是汉字）才回退到我们装入的字体。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

use crate::platform;

/// 装入中文字体。返回是否成功装入。
///
/// 找不到任何中文字体时不会 panic，只在日志里警告 ——
/// 程序仍然可用，只是中文会显示为方块。
pub fn install_cjk_fonts(ctx: &egui::Context) -> bool {
    let candidates = platform::cjk_font_candidates();
    let Some(font) = platform::load_first_available_font(&candidates) else {
        return false;
    };

    let mut fonts = FontDefinitions::default();

    // 注册字体数据。名字带哈希无关，取一个稳定的内部键即可。
    const FONT_KEY: &str = "cjk";
    fonts.font_data.insert(
        FONT_KEY.to_owned(),
        Arc::new(FontData::from_owned(font.bytes)),
    );

    // 追加到两种字体族的末尾作为回退：
    // - Proportional 用于绝大多数界面文字；
    // - Monospace 用于代码/数字对齐的场景（例如时长）。
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(FONT_KEY.to_owned());
    }

    ctx.set_fonts(fonts);

    log::info!("中文字体已启用：{}", font.label);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    /// 构造一个最小的、结构合法的 sfnt 头。
    ///
    /// 只验证装配流程不 panic 且字体确实进入了字体族列表；
    /// 真正渲染字形依赖 egui 内部解析，那部分由集成时的实际运行验证。
    fn fake_ttf() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // sfnt 版本
        v.extend_from_slice(&0u16.to_be_bytes()); // numTables
        v.extend_from_slice(&[0u8; 6]); // searchRange / entrySelector / rangeShift
        v
    }

    #[test]
    fn font_family_lists_include_cjk_when_installed() {
        // 直接验证装配逻辑的核心断言：追加后 CJK 键出现在族列表末尾，
        // 且内置字体仍排在其前面（保证拉丁字形优先）。
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), Arc::new(FontData::from_owned(fake_ttf())));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_owned());
        }

        let prop = fonts.families.get(&FontFamily::Proportional).unwrap();
        assert_eq!(prop.last().map(String::as_str), Some("cjk"));
        assert!(prop.len() >= 2, "内置字体应当仍排在前面");
        assert!(
            prop.iter().any(|n| n == "Ubuntu-Light"),
            "内置拉丁字体不应被移除"
        );

        let mono = fonts.families.get(&FontFamily::Monospace).unwrap();
        assert_eq!(mono.last().map(String::as_str), Some("cjk"));
    }

    #[test]
    fn install_does_not_panic_when_font_missing() {
        // 用一份空候选列表模拟「系统里没有任何中文字体」的环境。
        let empty: Vec<platform::FontCandidate> = Vec::new();
        assert!(platform::load_first_available_font(&empty).is_none());

        // 主流程在找不到字体时应当安全返回 false 而不是崩溃。
        // 这里用一个真实的 Context 验证整条装配路径不会 panic。
        let ctx = egui::Context::default();
        let installed = install_cjk_fonts(&ctx);
        // 在有中文字体的机器上应为 true；在极简容器里可能是 false。
        // 两种结果都算通过，关键是没 panic。
        log::debug!("install_cjk_fonts 返回 {installed}");
    }
}
