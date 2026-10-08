//! 播放链路的端到端集成测试。
//!
//! 这里验证的是「rodio 真的能把这台机器上的文件播起来」——
//! 单元测试无法覆盖这一点，因为它依赖真实的音频设备与解码器。
//!
//! 无声卡的环境（容器、远程桌面、CI）会安全跳过，不算失败。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use echo::playback::{AudioEngine, is_playable};

/// 生成一个最小合法 WAV（16-bit 单声道 8kHz 正弦波）。
fn write_wav(path: &PathBuf, seconds: u32) {
    const SR: u32 = 8_000;
    let samples = SR * seconds;
    let data_len = samples * 2;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&SR.to_le_bytes());
    buf.extend_from_slice(&(SR * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = i as f32 / SR as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
        buf.extend_from_slice(&((v * i16::MAX as f32) as i16).to_le_bytes());
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, buf).unwrap();
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("echo-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 若本机没有音频设备就跳过（返回 None），让 CI 上的无声环境不至于失败。
///
/// 返回 `mut` 绑定：`play` / `set_volume` 需要可变引用。
fn engine_or_skip() -> Option<AudioEngine> {
    match AudioEngine::new() {
        Ok(e) => Some(e),
        Err(err) => {
            eprintln!("跳过：本机没有可用音频设备（{err}）");
            None
        }
    }
}

#[test]
fn plays_a_real_file_and_position_advances() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };

    let dir = temp_dir("play");
    let wav = dir.join("tone.wav");
    write_wav(&wav, 3);

    engine.play(&wav, 0).expect("应当能开始播放");
    assert!(!engine.is_idle(), "开始播放后不应处于空闲状态");

    // 让音频线程跑一会儿，再确认播放位置真的在前进。
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut pos = Duration::ZERO;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(150));
        pos = engine.position();
        if pos > Duration::from_millis(200) {
            break;
        }
    }

    assert!(
        pos > Duration::from_millis(100),
        "播放位置应当前进，实际为 {pos:?}"
    );
    engine.stop();
}

#[test]
fn pause_and_resume_work() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };
    let dir = temp_dir("pause");
    let wav = dir.join("tone.wav");
    write_wav(&wav, 5);

    engine.play(&wav, 0).unwrap();
    assert!(!engine.is_paused());

    engine.pause();
    assert!(engine.is_paused());

    engine.resume();
    assert!(!engine.is_paused());

    engine.stop();
}

#[test]
fn seek_changes_position() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };
    let dir = temp_dir("seek");
    let wav = dir.join("tone.wav");
    write_wav(&wav, 5);

    engine.play(&wav, 0).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // 跳到 3 秒处。Decoder::try_from 会设置 byte_len，因此跳转应当被支持。
    engine.seek(Duration::from_secs(3)).expect("应当支持跳转");

    let after = engine.position();
    assert!(
        after >= Duration::from_secs(2),
        "跳转后位置应当在 3 秒附近，实际为 {after:?}"
    );
    engine.stop();
}

#[test]
fn volume_is_clamped() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };
    engine.set_volume(2.0);
    assert_eq!(engine.volume(), 1.0, "超上限的音量应当被夹到 1.0");
    engine.set_volume(-1.0);
    assert_eq!(engine.volume(), 0.0, "负音量应当被夹到 0.0");
    engine.set_volume(0.5);
    assert_eq!(engine.volume(), 0.5);
}

#[test]
fn unreachable_file_fails_cleanly() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };
    let bad = PathBuf::from("C:/绝对不存在的文件.mp3");
    let err = engine.play(&bad, 0).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("文件不存在") || msg.contains("暂不支持"),
        "应当给出可读的错误，实际：{msg}"
    );
}

#[test]
fn unsupported_format_is_rejected_before_decoding() {
    let Some(mut engine) = engine_or_skip() else {
        return;
    };
    let dir = temp_dir("unsupported");
    let opus = dir.join("x.opus");
    std::fs::write(&opus, b"not really opus").unwrap();

    assert!(!is_playable(&opus));
    let err = engine.play(&opus, 0).unwrap_err();
    assert!(
        err.to_string().contains("暂不支持"),
        "Opus 应当在解码前就被拦下，实际：{err}"
    );
}

// ---------------------------------------------------------------------------
// 曲库 + 封面的端到端验证
//
// 这里刻意不依赖任何外部素材或联网：现场生成音频与带封面的文件。
// ---------------------------------------------------------------------------

/// 生成一张最小的合法 PNG（用作内嵌封面）。
fn tiny_png() -> Vec<u8> {
    let mut img = image::RgbaImage::new(3, 3);
    for (i, px) in img.pixels_mut().enumerate() {
        *px = image::Rgba([(i * 40) as u8, 100, 200, 255]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// 手工拼一个合法的、**只含元数据**的 FLAC 文件骨架。
///
/// 之所以能这么做：FLAC 允许 `total_samples = 0`，也就是没有任何音频帧
/// 也依然是合法文件。换作 MP3 就不行 —— MPEG 解析器要求真的存在音频帧，
/// 手拼的假 MP3 会得到 `failed to parse Mpeg file`（实测如此）。
///
/// 这里只写出 `STREAMINFO` 与一个空的 `VORBIS_COMMENT` 块；
/// 标签与封面随后交给 lofty 自己的写入器填进去（见 `write_flac_with_cover`）。
fn write_flac_skeleton(path: &PathBuf) {
    /// 拼一个元数据块（最后一块的块头标志位要置 1）。
    fn meta_block(block_type: u8, last: bool, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(if last { 0x80 } else { 0x00 } | block_type);
        let len = body.len() as u32;
        out.extend_from_slice(&[
            ((len >> 16) & 0xff) as u8,
            ((len >> 8) & 0xff) as u8,
            (len & 0xff) as u8,
        ]);
        out.extend_from_slice(body);
        out
    }

    // STREAMINFO：34 字节，全零即可（total_samples = 0）。
    let streaminfo = [0u8; 34];
    // 空的 Vorbis Comment：4 字节厂商字符串长度(0) + 4 字节注释数(0)。
    let empty_vc = [0u8; 8];

    let mut file = Vec::new();
    file.extend_from_slice(b"fLaC");
    file.extend_from_slice(&meta_block(0, false, &streaminfo)); // 0 = STREAMINFO
    file.extend_from_slice(&meta_block(4, true, &empty_vc)); // 4 = VORBIS_COMMENT

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, file).unwrap();
}

/// 生成一个带真实内嵌封面的 FLAC 文件。
///
/// 封面用 **lofty 自己的写入器**写进去，而不是手工拼 base64 图片块 ——
/// 这样编码由库负责，测试验证的是真实的「写入 → 读回」往返，
/// 也更贴近用户实际的文件。
///
/// 注意：它没有音频帧，因此不能播放 —— 本测试只关心标签与封面。
fn write_flac_with_cover(path: &PathBuf, title: &str, artist: &str, png: &[u8]) {
    use lofty::config::WriteOptions;
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::tag::{Accessor, Tag};

    write_flac_skeleton(path);

    let mut tagged = lofty::read_from_path(path).expect("骨架应当能被解析");
    let tag_type = tagged.primary_tag_type();
    let mut tag = Tag::new(tag_type);
    tag.set_title(title.to_owned());
    tag.set_artist(artist.to_owned());
    tag.push_picture(
        Picture::unchecked(png.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .build(),
    );
    tagged.insert_tag(tag);
    tagged
        .save_to_path(path, WriteOptions::default())
        .expect("写入标签应当成功");
}

/// 标准 base64 编码（带 `=` 填充，不换行）。
///
/// 自己实现是为了不给测试引入额外依赖；正确性由
/// `base64_matches_rfc4648_vectors` 用 RFC 4648 官方测试向量守着。
#[allow(dead_code)]
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[test]
fn library_scan_stores_tracks_and_covers() {
    use echo::db::Db;
    use echo::metadata::is_audio_path;

    let dir = temp_dir("scan");
    write_wav(&dir.join("a.wav"), 1);
    write_wav(&dir.join("sub").join("b.wav"), 1);
    // 非音频文件必须被忽略。
    std::fs::write(dir.join("readme.txt"), b"ignore").unwrap();

    let db = Db::open_in_memory().unwrap();
    let events = std::cell::RefCell::new(Vec::new());
    let summary = echo::library::run_scan(std::slice::from_ref(&dir), &db, &|ev| {
        events.borrow_mut().push(ev);
    })
    .expect("扫描应当成功");

    assert_eq!(summary.added, 2, "应当入库两个 wav（嵌套目录也要走到）");
    assert_eq!(db.track_count().unwrap(), 2);
    assert!(!events.borrow().is_empty(), "应当上报过进度事件");

    // 重扫：文件未变，应当全部走快速路径，且不重复入库。
    let second = echo::library::run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();
    assert_eq!(second.skipped, 2, "未变更的文件应当被跳过");
    assert_eq!(second.added, 0);
    assert_eq!(second.removed, 0, "跳过的文件不能被误删");
    assert_eq!(db.track_count().unwrap(), 2);

    // 读出来的曲目应当能定位到真实文件。
    for t in db.all_tracks_sorted().unwrap() {
        assert!(t.path.exists(), "库里记录的路径应当真实存在：{:?}", t.path);
        assert!(is_audio_path(&t.path));
        assert!(t.duration_ms > 0, "生成的 wav 应当有时长");
    }
}

#[test]
fn base64_matches_rfc4648_vectors() {
    // 用 RFC 4648 的官方测试向量校验 base64 实现。
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn embedded_cover_is_extracted_stored_and_decoded() {
    use echo::covers::{CoverCache, decode_to_color_image};
    use echo::db::Db;
    use echo::domain::CoverArt;

    let dir = temp_dir("cover");
    let flac = dir.join("song.flac");
    let png = tiny_png();
    write_flac_with_cover(&flac, "声波测试", "张伟", &png);

    // 1) lofty 能读出文本标签与内嵌封面。
    let meta = echo::metadata::read_track(&flac).expect("应当能解析这个 FLAC 文件");
    assert_eq!(meta.title, "声波测试", "中文标题应当被正确读出");
    assert_eq!(meta.artist, "张伟");
    let cover = meta.cover.as_ref().expect("应当读到内嵌封面");
    assert_eq!(cover.mime, "image/png", "MIME 应当被规范化");
    assert_eq!(cover.bytes[..], png[..], "读出的封面字节应当与写入的一致");
    assert!(!cover.hash.is_empty());

    // 2) 落库后能原样取回（按内容哈希寻址）。
    let db = Db::open_in_memory().unwrap();
    db.put_cover(&CoverArt {
        hash: cover.hash.clone(),
        mime: cover.mime.clone(),
        bytes: cover.bytes.clone(),
    })
    .unwrap();
    let (mime, bytes) = db.get_cover(&cover.hash).unwrap().expect("应当能取回封面");
    assert_eq!(mime, "image/png");
    assert_eq!(bytes[..], png[..], "取回的封面字节应当原样一致");

    // 3) 解码成 egui 纹理并缓存。
    let ctx = egui::Context::default();
    let mut cache = CoverCache::new();
    let tex = cache
        .get_for_track(&ctx, &db, Some(&cover.hash))
        .expect("应当能解码出纹理");
    assert_eq!(tex.size(), [3, 3]);

    // 4) 同一个哈希重复请求应当命中缓存，而不是重复解码。
    assert!(cache.get_for_track(&ctx, &db, Some(&cover.hash)).is_some());
    assert_eq!(cache.len(), 1, "同一张封面只应缓存一份");

    // 5) 直接解码的接口也应当给出一致结果。
    let img = decode_to_color_image(&png).expect("应当能解码");
    assert_eq!(img.size, [3, 3]);
}

#[test]
fn scanned_track_can_be_played() {
    use echo::db::Db;

    // 这条把「扫描 → 入库 → 取回 → 播放」串起来，
    // 正是用户点击列表后实际会走的完整路径。
    let Some(mut engine) = engine_or_skip() else {
        return;
    };

    let dir = temp_dir("scanplay");
    write_wav(&dir.join("playable.wav"), 3);

    let db = Db::open_in_memory().unwrap();
    echo::library::run_scan(std::slice::from_ref(&dir), &db, &|_| {}).unwrap();

    let tracks = db.all_tracks_sorted().unwrap();
    assert_eq!(tracks.len(), 1);

    engine
        .play(&tracks[0].path, 0)
        .expect("扫描入库的曲目应当能播放");
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        engine.position() > Duration::from_millis(50),
        "播放位置应当前进"
    );
    engine.stop();
}
