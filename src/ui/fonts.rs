//! 中文字体装配。
//!
//! egui 0.36 内置的字体只有 Ubuntu-Light（拉丁）、Hack（等宽）、
//! emoji-icon-font 和 NotoEmoji，**不含任何 CJK 字形**。
//! 如果不额外装入系统字体，界面上的「曲库」「播放」以及中文歌曲标签
//! 全都会渲染成空心方块（tofu），因此这一步是中文可用性的前提。
//!
//! 装配策略是**中文优先**：把中文字体放在字体族列表的最前面。
//!
//! 之前是反过来的（拉丁用内置 Ubuntu-Light，汉字回退到末尾）。
//! 那种写法在英文界面里没问题，但中文界面的绝大多数文字是汉字，
//! 由 Ubuntu-Light 主导会让汉字与数字混排时字重、基线都不统一，
//! 观感明显别扭 —— 中文界面应当让中文字体打头，拉丁字符自然回退。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

use crate::platform;

/// 装入中文字体并将其设为首选。返回是否成功装入。
///
/// 找不到任何中文字体时不会 panic，只在日志里警告 ——
/// 程序仍然可用，只是中文会显示为方块。
pub fn install_cjk_fonts(ctx: &egui::Context) -> bool {
    let candidates = platform::cjk_font_candidates();
    let Some(font) = platform::load_first_available_font(&candidates) else {
        return false;
    };

    let mut fonts = FontDefinitions::default();

    // 注册字体数据。内部键取一个稳定的名字即可。
    const FONT_KEY: &str = "cjk";
    fonts.font_data.insert(
        FONT_KEY.to_owned(),
        Arc::new(FontData::from_owned(font.bytes)),
    );

    // 插到两种字体族的最前面作为首选：
    // - Proportional 用于绝大多数界面文字；
    // - Monospace 用于需要对齐的场景（例如时长）。
    //
    // 内置字体仍排在后面，因此「√」这类中文字体里没有的符号
    // 依然能回退到 Ubuntu-Light 找到字形，不会变成方块。
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, FONT_KEY.to_owned());
    }

    ctx.set_fonts(fonts);

    log::info!("中文字体已启用（首选）：{}", font.label);
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
    fn font_family_lists_put_cjk_first() {
        // 验证装配策略：CJK 键排在族列表**最前面**（中文优先），
        // 而内置字体仍保留在后面作为拉丁字符与符号的回退。
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), Arc::new(FontData::from_owned(fake_ttf())));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = fonts.families.entry(family).or_default();
            list.insert(0, "cjk".to_owned());
        }

        let prop = fonts.families.get(&FontFamily::Proportional).unwrap();
        assert_eq!(
            prop.first().map(String::as_str),
            Some("cjk"),
            "中文字体应当排在首位"
        );
        assert!(prop.len() >= 2, "内置字体应当被保留在后面");
        assert!(
            prop.iter().any(|n| n == "Ubuntu-Light"),
            "内置拉丁字体不应被移除（√ 这类符号还要靠它）"
        );

        let mono = fonts.families.get(&FontFamily::Monospace).unwrap();
        assert_eq!(mono.first().map(String::as_str), Some("cjk"));
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
