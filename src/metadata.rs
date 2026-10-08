//! 元数据层：用 lofty 读取标签与内嵌封面。
//!
//! 这一层完全跨平台，不含任何平台判定。
//!
//! 几个 lofty 0.25 的要点（与旧版教程差异较大）：
//! - `TaggedFile` 的 `properties()` / `primary_tag()` 来自 **trait**，
//!   必须 `use lofty::file::{AudioFile, TaggedFileExt}` 才能调用；
//! - `title()` / `artist()` 等来自 `Accessor` trait，返回 `Option<Cow<'_, str>>`；
//! - **没有 `year()`**，要用 `tag.date().map(|d| d.year)`；
//! - **没有 `album_artist()`**，要用 `tag.get_string(ItemKey::AlbumArtist)`；
//! - 封面**不在** `TagItem`/`ItemValue` 上，而在 `Tag::pictures()` / `get_picture_type()`。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};

use crate::domain::{CoverArt, TrackMeta};

/// 支持的音频扩展名（小写、不含点）。
///
/// 扫描时先用它过滤，避免对非音频文件做昂贵的解析。
/// 注意 Opus 在列：lofty 能读它的标签，但 rodio 0.22 无法解码，
/// 因此它会出现在曲库里但标记为不可播放（见 `playback::is_playable`）。
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "wave", "m4a", "m4b", "mp4", "aac", "ogg", "oga", "opus", "wma", "aiff",
    "aif", "ape", "mpc", "wv", "spx",
];

/// 扩展名是否属于音频（大小写不敏感）。
pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 读取一个音频文件的元数据。
///
/// 即使标签缺失也会返回 `Ok`（只是各字段为空），只有文件无法解析时才返回 `Err`。
pub fn read_track(path: &Path) -> Result<TrackMeta> {
    let tagged = Probe::open(path)
        .with_context(|| format!("无法打开 {}", path.display()))?
        .options(ParseOptions::new().read_properties(true))
        .guess_file_type()
        .with_context(|| format!("无法识别文件类型 {}", path.display()))?
        .read()
        .with_context(|| format!("无法解析音频文件 {}", path.display()))?;

    // 优先主标签，退化到第一个可用标签。
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let mut meta = TrackMeta {
        path: path.to_path_buf(),
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        track_no: None,
        disc_no: None,
        year: None,
        genre: String::new(),
        duration_ms: 0,
        sample_rate: None,
        bitrate: None,
        channels: None,
        cover: None,
        file_size: 0,
        mtime: 0,
    };

    if let Some(tag) = tag {
        meta.title = cow_to_string(tag.title());
        meta.artist = cow_to_string(tag.artist());
        meta.album = cow_to_string(tag.album());
        meta.genre = cow_to_string(tag.genre());
        meta.track_no = tag.track();
        meta.disc_no = tag.disk();
        // Accessor 没有 year()，日期是 Timestamp 结构体。
        meta.year = tag.date().map(|d| d.year);
        // 专辑艺术家不在 Accessor 上，走 ItemKey。
        meta.album_artist = tag
            .get_string(ItemKey::AlbumArtist)
            .map(str::to_owned)
            .unwrap_or_default();
        meta.cover = read_cover(tag);
    }

    let props = tagged.properties();
    meta.duration_ms = props.duration().as_millis() as u64;
    meta.sample_rate = props.sample_rate();
    meta.bitrate = props.audio_bitrate().or_else(|| props.overall_bitrate());
    meta.channels = props.channels();

    let (file_size, mtime) = file_signature(path)?;
    meta.file_size = file_size;
    meta.mtime = mtime;

    Ok(meta)
}

/// 取内嵌封面：优先「正面封面」，否则退回第一张图。
fn read_cover(tag: &lofty::tag::Tag) -> Option<CoverArt> {
    let picture = tag
        .get_picture_type(PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;
    let bytes = picture.data().to_vec();
    if bytes.is_empty() {
        return None;
    }
    Some(CoverArt {
        hash: blake3::hash(&bytes).to_hex().to_string(),
        mime: mime_to_string(picture.mime_type()),
        bytes,
    })
}

/// `MimeType` 转成字符串。
///
/// 已知类型由 lofty 给出 `image/png` 这类标准形式；`Unknown` 会把标签里
/// 的原始字符串透传出来，可能是 `jpg` 这种简写，这里统一补成 `image/` 前缀。
fn mime_to_string(mime: Option<&lofty::picture::MimeType>) -> String {
    let Some(m) = mime else {
        return "image/jpeg".to_string();
    };
    let s = m.as_str().trim();
    if s.is_empty() {
        return "image/jpeg".to_string();
    }
    if s.to_ascii_lowercase().starts_with("image/") {
        s.to_string()
    } else {
        format!("image/{}", s.to_ascii_lowercase())
    }
}

fn cow_to_string(value: Option<std::borrow::Cow<'_, str>>) -> String {
    value.map(|c| c.trim().to_owned()).unwrap_or_default()
}

/// 读取 (文件大小, 修改时间)。修改时间用 Unix 秒，跨平台一致。
pub fn file_signature(path: &Path) -> Result<(u64, i64)> {
    let md =
        std::fs::metadata(path).with_context(|| format!("无法读取文件属性 {}", path.display()))?;
    let size = md.len();
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(|| {
            // 极少数文件系统不支持修改时间，退化为「当前时间」，
            // 效果是它每次扫描都会被重新解析，但不会出错。
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    Ok((size, mtime))
}

/// 生成一个最小的合法 WAV 文件（单声道 8kHz 正弦波），仅测试用。
///
/// 有了它就能在不依赖任何外部素材的情况下验证：
/// lofty 能解析、rodio 能解码、扫描器能入库。
#[cfg(test)]
pub fn write_test_wav(path: &Path, seconds: u32) -> Result<()> {
    use std::io::Write as _;

    const SAMPLE_RATE: u32 = 8_000;
    const FREQ: f32 = 440.0;
    let samples = SAMPLE_RATE * seconds;
    let data_len = samples * 2; // 16-bit 单声道

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // fmt 块长度
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // 声道数
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // 字节率
    buf.extend_from_slice(&2u16.to_le_bytes()); // 块对齐
    buf.extend_from_slice(&16u16.to_le_bytes()); // 位深
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let v = (t * FREQ * std::f32::consts::TAU).sin() * 0.3;
        buf.extend_from_slice(&((v * i16::MAX as f32) as i16).to_le_bytes());
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    Ok(())
}

/// 一个便于单测的「构造一个带标签的临时音频文件」辅助函数。
#[cfg(test)]
pub fn write_tagged_test_wav(path: &Path, title: &str, artist: &str, album: &str) -> Result<()> {
    write_test_wav(path, 1)?;
    // 用 lofty 把标签写进去，确保后续读回时确实有内容。
    use lofty::config::WriteOptions;
    use lofty::tag::Tag;
    let mut tagged = Probe::open(path)?.guess_file_type()?.read()?;
    let tag_type = tagged.primary_tag_type();
    let mut tag = Tag::new(tag_type);
    tag.set_title(title.to_owned());
    tag.set_artist(artist.to_owned());
    tag.set_album(album.to_owned());
    tag.set_track(1);
    tagged.insert_tag(tag);
    tagged
        .save_to_path(path, WriteOptions::default())
        .context("写入测试标签失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-meta-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extension_gate_accepts_audio_rejects_others() {
        assert!(is_audio_path(Path::new("a.mp3")));
        assert!(is_audio_path(Path::new("a.MP3")));
        assert!(is_audio_path(Path::new("a.FlAc")));
        assert!(is_audio_path(Path::new("dir/b.m4a")));
        assert!(is_audio_path(Path::new("a.opus")));

        assert!(!is_audio_path(Path::new("a.txt")));
        assert!(!is_audio_path(Path::new("a.jpg")));
        assert!(!is_audio_path(Path::new("noext")));
        assert!(!is_audio_path(Path::new("a.mp3.bak")));
    }

    #[test]
    fn reads_duration_from_generated_wav() {
        let dir = temp_dir("dur");
        let wav = dir.join("tone.wav");
        write_test_wav(&wav, 1).unwrap();

        let meta = read_track(&wav).unwrap();
        // 1 秒的采样，时长应当接近 1000ms。
        assert!(
            meta.duration_ms >= 950 && meta.duration_ms <= 1050,
            "时长异常: {}ms",
            meta.duration_ms
        );
        assert_eq!(meta.sample_rate, Some(8_000));
        assert_eq!(meta.channels, Some(1));
        assert!(meta.file_size > 44);
        assert!(meta.mtime > 0);
    }

    #[test]
    fn missing_tags_degrade_to_empty_strings() {
        let dir = temp_dir("notags");
        let wav = dir.join("bare.wav");
        write_test_wav(&wav, 1).unwrap();

        let meta = read_track(&wav).unwrap();
        assert_eq!(meta.title, "");
        assert_eq!(meta.artist, "");
        assert_eq!(meta.album, "");
        assert_eq!(meta.year, None);
        assert_eq!(meta.track_no, None);
        assert!(meta.cover.is_none());
        // 标题为空时展示层应回退到文件名。
        let track = crate::domain::Track {
            id: 1,
            path: meta.path.clone(),
            title: meta.title.clone(),
            artist: meta.artist.clone(),
            album: meta.album.clone(),
            album_artist: String::new(),
            track_no: None,
            disc_no: None,
            year: None,
            genre: String::new(),
            duration_ms: meta.duration_ms,
            sample_rate: meta.sample_rate,
            bitrate: meta.bitrate,
            channels: meta.channels,
            cover_hash: None,
            file_size: meta.file_size,
            mtime: meta.mtime,
        };
        assert_eq!(track.display_title(), "bare");
    }

    #[test]
    fn reads_tags_written_by_lofty() {
        let dir = temp_dir("tags");
        let wav = dir.join("tagged.wav");
        write_tagged_test_wav(&wav, "声波测试", "张伟", "测试专辑").unwrap();

        let meta = read_track(&wav).unwrap();
        assert_eq!(meta.title, "声波测试");
        assert_eq!(meta.artist, "张伟");
        assert_eq!(meta.album, "测试专辑");
        assert_eq!(meta.track_no, Some(1));
        assert!(meta.duration_ms > 0);
    }

    #[test]
    fn nonexistent_file_is_an_error() {
        let dir = temp_dir("missing");
        let missing = dir.join("nope.wav");
        assert!(read_track(&missing).is_err());
    }

    #[test]
    fn non_audio_file_fails_to_parse() {
        let dir = temp_dir("bogus");
        let txt = dir.join("fake.mp3");
        // 用 as_bytes 而不是字节字符串字面量：后者只允许 ASCII。
        std::fs::write(&txt, "这不是音频".as_bytes()).unwrap();
        // 扩展名虽在允许列表内，但内容不是音频，解析应当失败而不是 panic。
        assert!(read_track(&txt).is_err());
    }

    #[test]
    fn file_signature_changes_with_content() {
        let dir = temp_dir("sig");
        let f = dir.join("x.bin");
        std::fs::write(&f, b"12345").unwrap();
        let (size1, _) = file_signature(&f).unwrap();
        assert_eq!(size1, 5);

        std::fs::write(&f, b"1234567890").unwrap();
        let (size2, _) = file_signature(&f).unwrap();
        assert_eq!(size2, 10);
    }

    #[test]
    fn mime_normalization() {
        assert_eq!(mime_to_string(None), "image/jpeg");
    }
}
