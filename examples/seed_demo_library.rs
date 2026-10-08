//! 造一批带内嵌封面的测试音乐，并扫描进曲库。
//!
//! 只用于**本地验收界面观感** —— 曲库为空时界面只能显示空状态，
//! 没法判断设计好坏。
//!
//! ```text
//! cargo run --example seed_demo_library --features demo-data
//! ```
//!
//! 生成的 FLAC 只有元数据、没有音频帧（见 `metadata::write_flac_skeleton`
//! 的说明），所以它们**能显示但不能播放**。

use std::path::PathBuf;

use echo::db::Db;
use echo::library;

/// 一张对角渐变的纯色封面，用来区分不同专辑。
fn cover_png(r: u8, g: u8, b: u8) -> Vec<u8> {
    let size = 320u32;
    let mut img = image::RgbaImage::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let t = ((x + y) as f32 / (size * 2) as f32).clamp(0.0, 1.0);
        let f = |c: u8| (c as f32 * (0.45 + 0.55 * t)).min(255.0) as u8;
        *px = image::Rgba([f(r), f(g), f(b), 255]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

struct Demo {
    file: &'static str,
    title: &'static str,
    artist: &'static str,
    album: &'static str,
    track: u32,
    color: (u8, u8, u8),
}

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(r"D:\Echo\.tools\demo-music");
    let _ = std::fs::remove_dir_all(&dir);

    // 三张「专辑」，每张共用一张封面。
    let demos = [
        Demo {
            file: "01-opening.flac",
            title: "序曲",
            artist: "回声乐队",
            album: "深夜频率",
            track: 1,
            color: (60, 196, 214),
        },
        Demo {
            file: "02-neon-rain.flac",
            title: "霓虹与雨",
            artist: "回声乐队",
            album: "深夜频率",
            track: 2,
            color: (60, 196, 214),
        },
        Demo {
            file: "03-drift.flac",
            title: "漂流",
            artist: "回声乐队",
            album: "深夜频率",
            track: 3,
            color: (60, 196, 214),
        },
        Demo {
            file: "04-threshold.flac",
            title: "Threshold",
            artist: "Kite & Co.",
            album: "Quiet Machines",
            track: 1,
            color: (232, 149, 84),
        },
        Demo {
            file: "05-paper-lanterns.flac",
            title: "Paper Lanterns",
            artist: "Kite & Co.",
            album: "Quiet Machines",
            track: 2,
            color: (232, 149, 84),
        },
        Demo {
            file: "06-long-night.flac",
            title: "漫长的夜",
            artist: "末班车",
            album: "单曲",
            track: 1,
            color: (162, 132, 232),
        },
    ];

    let mut album_covers: std::collections::HashMap<&str, Vec<u8>> =
        std::collections::HashMap::new();

    for d in &demos {
        let path = dir.join(d.file);
        echo::metadata::write_test_flac(&path, d.title, d.artist, d.album, d.track)?;

        let cover = album_covers
            .entry(d.album)
            .or_insert_with(|| cover_png(d.color.0, d.color.1, d.color.2));
        echo::metadata::embed_cover(&path, cover)?;
    }

    // 扫描进真实曲库（用户数据目录里的 library.db）。
    let paths = echo::platform::paths();
    let db = Db::open(&paths.db_path())?;
    db.add_scan_root(&dir)?;

    let summary = library::run_scan(std::slice::from_ref(&dir), &db, &|_| {})?;
    println!("扫描完成：{summary:?}");
    println!(
        "曲库共 {} 首，封面 {} 张",
        db.track_count()?,
        db.cover_count()?
    );
    println!("音乐目录：{}", dir.display());
    println!("数据库：{}", paths.db_path().display());
    Ok(())
}
