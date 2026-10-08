//! 封面层：把内嵌图片字节解码成 egui 纹理，并按内容哈希缓存。
//!
//! 这一层完全跨平台。两个关键设计：
//! 1. **按哈希缓存纹理**：同一张专辑图在列表里出现几十次也只会解码一次；
//! 2. **失败也缓存**：损坏或超大图片记为「无封面」，不会每帧重试拖垮界面。

// 缓存与查询接口里有一部分（len / is_empty / get_or_decode）主要供测试使用，
// 在生产路径上暂时用不到，这里统一豁免，避免满屏 dead_code 警告。
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::db::Db;

/// 纹理缓存上限。超出后按先进先出淘汰（专辑图访问局部性强，够用）。
const CACHE_CAPACITY: usize = 64;

/// 封面纹理缓存。
pub struct CoverCache {
    /// 键为封面哈希；`None` 表示这张图解码失败，无需再试。
    textures: HashMap<String, Option<TextureHandle>>,
    /// 淘汰顺序。
    order: VecDeque<String>,
}

impl Default for CoverCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    /// 清空缓存（重新扫描后用，避免显示已变更的旧封面）。
    pub fn clear(&mut self) {
        self.textures.clear();
        self.order.clear();
    }

    /// 取（必要时解码）某首歌的封面纹理。
    ///
    /// `cover_hash` 为 `None` 时直接返回 `None`，不做任何数据库访问。
    /// 返回克隆而非引用：`TextureHandle` 内部是 `Arc`，克隆只是引用计数加一，
    /// 但能让调用方在 UI 闭包里自由使用，不必与 `&mut self` 的借用打架。
    pub fn get_for_track(
        &mut self,
        ctx: &egui::Context,
        db: &Db,
        cover_hash: Option<&str>,
    ) -> Option<TextureHandle> {
        let hash = cover_hash?;

        if !self.textures.contains_key(hash) {
            let decoded = db
                .get_cover(hash)
                .ok()
                .flatten()
                .and_then(|(_mime, bytes)| decode_to_color_image(&bytes));
            let handle = decoded.map(|img| ctx.load_texture(hash, img, TextureOptions::LINEAR));
            self.insert(hash.to_string(), handle);
        }

        self.textures.get(hash).and_then(|t| t.clone())
    }

    /// 直接由字节解码并缓存，主要供测试与外部素材使用。
    pub fn get_or_decode(
        &mut self,
        ctx: &egui::Context,
        hash: &str,
        bytes: &[u8],
    ) -> Option<TextureHandle> {
        if !self.textures.contains_key(hash) {
            let handle = decode_to_color_image(bytes)
                .map(|img| ctx.load_texture(hash.to_string(), img, TextureOptions::LINEAR));
            self.insert(hash.to_string(), handle);
        }
        self.textures.get(hash).and_then(|t| t.clone())
    }

    fn insert(&mut self, key: String, value: Option<TextureHandle>) {
        // 已在缓存中则只更新值，不改变淘汰顺序。
        if self.textures.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_CAPACITY {
            if let Some(old) = self.order.pop_front() {
                self.textures.remove(&old);
            }
        }
    }
}

/// 把任意常见图片格式的字节解码为 egui 的 `ColorImage`。
///
/// 这里**不用** egui_extras 的文件加载器：内嵌封面没有文件路径，
/// 只能自己用 `image` crate 解码（这正是 Cargo.toml 里显式开启
/// png/jpeg/gif/bmp 解码特性的原因）。
pub fn decode_to_color_image(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let raw = rgba.as_raw();
    // ColorImage::from_rgba_unmultiplied 在长度不符时会 panic，
    // 这里先校验，坏数据只当「无封面」处理。
    if raw.len() != w * h * 4 {
        log::warn!(
            "封面像素长度异常：{raw_len} != {w}x{h}x4",
            raw_len = raw.len()
        );
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用 image crate 现场编码一张 PNG，避免测试依赖外部素材。
    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_png_to_color_image() {
        let bytes = png_bytes(4, 3);
        let img = decode_to_color_image(&bytes).expect("应当解码成功");
        assert_eq!(img.size, [4, 3]);
        assert_eq!(img.pixels.len(), 12);
    }

    #[test]
    fn decodes_jpeg_to_color_image() {
        let mut img = image::RgbImage::new(8, 8);
        for px in img.pixels_mut() {
            *px = image::Rgb([200, 100, 50]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        let decoded = decode_to_color_image(&out.into_inner()).expect("JPEG 应当解码成功");
        assert_eq!(decoded.size, [8, 8]);
    }

    #[test]
    fn garbage_bytes_return_none_without_panicking() {
        assert!(decode_to_color_image("这不是图片".as_bytes()).is_none());
        assert!(decode_to_color_image(&[]).is_none());
        // 截断的 PNG 头也应当安全返回 None。
        let mut bytes = png_bytes(4, 4);
        bytes.truncate(20);
        assert!(decode_to_color_image(&bytes).is_none());
    }

    #[test]
    fn cache_evicts_oldest_and_records_failures() {
        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        assert!(cache.is_empty());

        let good = png_bytes(2, 2);
        assert!(cache.get_or_decode(&ctx, "good", &good).is_some());
        assert_eq!(cache.len(), 1);

        // 坏图：记入缓存但值为 None，下次不会再尝试解码。
        assert!(cache.get_or_decode(&ctx, "bad", b"nope").is_none());
        assert_eq!(cache.len(), 2, "失败结果也应当被缓存");

        // 命中缓存时返回同一个纹理。
        let again = cache.get_or_decode(&ctx, "good", &good);
        assert!(again.is_some());
    }

    #[test]
    fn cache_respects_capacity() {
        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        let bytes = png_bytes(2, 2);
        for i in 0..(CACHE_CAPACITY + 10) {
            let key = format!("k{i}");
            cache.get_or_decode(&ctx, &key, &bytes);
        }
        assert!(
            cache.len() <= CACHE_CAPACITY,
            "缓存不应超过上限，实际 {}",
            cache.len()
        );
        // 最早的键应当已被淘汰。
        assert!(!cache.textures.contains_key("k0"));
    }

    #[test]
    fn clear_empties_cache() {
        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        cache.get_or_decode(&ctx, "x", &png_bytes(2, 2));
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn missing_hash_returns_none() {
        let db = Db::open_in_memory().unwrap();
        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        assert!(cache.get_for_track(&ctx, &db, None).is_none());
        // 哈希不存在于数据库时也应当安全返回 None。
        assert!(cache.get_for_track(&ctx, &db, Some("不存在")).is_none());
    }

    #[test]
    fn loads_cover_from_database() {
        use crate::domain::CoverArt;
        let db = Db::open_in_memory().unwrap();
        let bytes = png_bytes(6, 6);
        db.put_cover(&CoverArt {
            hash: "h1".into(),
            mime: "image/png".into(),
            bytes,
        })
        .unwrap();

        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        let tex = cache.get_for_track(&ctx, &db, Some("h1"));
        assert!(tex.is_some(), "应当能从库里读出并解码封面");
        assert_eq!(tex.unwrap().size(), [6, 6]);
    }

    #[test]
    fn corrupt_cover_in_database_is_cached_as_missing() {
        use crate::domain::CoverArt;
        let db = Db::open_in_memory().unwrap();
        db.put_cover(&CoverArt {
            hash: "broken".into(),
            mime: "image/png".into(),
            bytes: vec![0, 1, 2, 3],
        })
        .unwrap();

        let ctx = egui::Context::default();
        let mut cache = CoverCache::new();
        assert!(cache.get_for_track(&ctx, &db, Some("broken")).is_none());
        // 已记入缓存，避免每帧重试。
        assert_eq!(cache.len(), 1);
    }
}
