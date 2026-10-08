//! 播放层：封装 rodio，对外只暴露与平台无关的 API。
//!
//! Windows / macOS / Linux 的音频后端（WASAPI / CoreAudio / ALSA）由 rodio
//! 内部选择，因此这一层不需要任何平台判定 —— 这正是跨平台架构里
//! 「平台差异不进核心」的体现。
//!
//! # rodio 0.22 的 API 与旧版差异很大
//!
//! - **`Sink` 已改名为 `Player`**，**`OutputStream` 改名为 `MixerDeviceSink`**；
//! - 入口是 `DeviceSinkBuilder::open_default_sink()`，再 `Player::connect_new(sink.mixer())`；
//! - `Player` 的所有控制方法都取 `&self`，因此可以放在 `&mut self` 的 UI 里随便调；
//! - **析构顺序有语义**：`MixerDeviceSink` 一旦被丢弃，播放立即停止。
//!   所以 `AudioEngine` 把 sink 放在第一个字段，保证它比 player 活得久。

// 对外暴露的是完整的播放控制接口，其中一部分（stop / has_finished /
// device_config_debug 等）暂时没有被界面调用，保留以便后续功能使用。
#![allow(dead_code)]

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

/// rodio 0.22 能解码的扩展名。
///
/// 注意与 `metadata::AUDIO_EXTENSIONS` 的区别：lofty 能**读标签**的格式更多
/// （例如 Opus、WMA），但 rodio 无法**解码**它们。两者不一致时以本列表为准。
pub const PLAYABLE_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "wave", "m4a", "m4b", "mp4", "aac", "ogg", "oga",
];

/// 该文件是否可播放。不可播放的文件仍会出现在曲库里，只是点击时给出提示。
pub fn is_playable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PLAYABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 音频引擎。
///
/// 字段顺序**不可调整**：`sink` 必须在 `player` 之前声明，
/// 这样 Rust 的字段析构顺序（按声明顺序）保证音频设备比播放句柄后释放。
pub struct AudioEngine {
    /// 持有操作系统音频设备；丢弃即停止播放。
    sink: MixerDeviceSink,
    player: Player,
    volume: f32,
    /// 每次开始新曲目时自增，用来识别并忽略「上一个曲目」遗留的结束回调。
    generation: Arc<AtomicUsize>,
    /// 自动切歌信号：`(代际, 刚播完的曲目下标)`。
    ///
    /// 带上代际是必需的：曲目结束的回调在音频线程上执行，而切歌发生在 UI 线程，
    /// 两者存在竞态 —— 若只存下标，上一首遗留的回调可能在切歌**之后**才写入，
    /// 导致界面莫名跳到错误的曲目。取用时代际不符即丢弃。
    pending: Arc<Mutex<Option<(usize, usize)>>>,
}

impl AudioEngine {
    /// 打开默认音频设备。
    ///
    /// 无声卡、远程桌面或容器环境下会返回 `Err` —— 调用方应当把它当作
    /// 可降级状态处理（曲库仍可浏览），而不是让它崩掉整个程序。
    pub fn new() -> Result<Self> {
        let sink = DeviceSinkBuilder::open_default_sink()
            .context("无法打开默认音频设备（可能没有可用声卡）")?;
        // 丢弃时打印的提示对我们没用，关掉以免污染日志。
        let mut sink = sink;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Self {
            sink,
            player,
            volume: 1.0,
            generation: Arc::new(AtomicUsize::new(0)),
            pending: Arc::new(Mutex::new(None)),
        })
    }

    /// 当前音量（0.0 ~ 1.0）。
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// 设置音量。超出 [0,1] 的值会被夹紧。
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// 是否已没有任何声音在排队（含尚未开始播放的情况）。
    pub fn is_idle(&self) -> bool {
        self.player.empty()
    }

    /// 当前播放位置。
    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    /// 开始播放指定曲目，`index` 是它在播放队列里的下标，
    /// 用于在播放结束时告诉 UI「该切到哪一首」。
    ///
    /// 会先清空队列，因此连点两首歌不会叠着一起响。
    pub fn play(&mut self, path: &Path, index: usize) -> Result<()> {
        if !is_playable(path) {
            return Err(anyhow!(
                "暂不支持播放 {} 格式（rodio 0.22 无法解码该编码）",
                path.extension().and_then(|e| e.to_str()).unwrap_or("未知")
            ));
        }
        if !path.exists() {
            return Err(anyhow!("文件不存在：{}", path.display()));
        }

        self.player.clear();

        // 让上一首遗留的回调失效：它捕获的 generation 会与当前值不符。
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut p) = self.pending.lock() {
            *p = None;
        }

        let file = File::open(path).with_context(|| format!("无法打开文件 {}", path.display()))?;
        // 用 try_from(File)：它会顺带设置 byte_len，而拖动进度条依赖这个信息。
        let source =
            Decoder::try_from(file).with_context(|| format!("无法解码音频 {}", path.display()))?;

        self.player.append(source);
        self.player.set_volume(self.volume);

        // 在曲目末尾挂一个回调，用来通知「本曲已播完」。
        // 回调运行在**音频线程**上，因此只允许碰这个互斥量，绝不触碰 UI 状态。
        let gen_arc = Arc::clone(&self.generation);
        let pending_arc = Arc::clone(&self.pending);
        self.player
            .append(rodio::source::EmptyCallback::new(Box::new(move || {
                let current = gen_arc.load(Ordering::SeqCst);
                if let Ok(mut p) = pending_arc.lock() {
                    // 代际校验与写入在同一次持锁内完成，因此不存在
                    // 「检查通过后、写入前被切歌」的窗口。
                    if current == generation {
                        *p = Some((generation, index));
                    }
                }
            })));

        self.player.play();
        Ok(())
    }

    /// 取出「刚播完的曲目下标」，没有则返回 `None`。每帧调用一次即可。
    ///
    /// 代际不符的信号（上一首遗留的回调）会被丢弃并返回 `None`。
    pub fn take_finished_index(&self) -> Option<usize> {
        let mut p = self.pending.lock().ok()?;
        match *p {
            Some((signal_gen, index)) => {
                *p = None;
                // `gen` 在 edition 2024 是保留字，所以用 signal_gen。
                if signal_gen == self.generation.load(Ordering::SeqCst) {
                    Some(index)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 是否有待处理的自动切歌（不消费信号）。
    pub fn has_finished(&self) -> bool {
        self.pending.lock().map(|p| p.is_some()).unwrap_or(false)
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
    }

    /// 在播放/暂停之间切换。
    pub fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    /// 拖动进度条。时长未知时 rodio 会返回 `NotSupported`，此处不视为致命错误。
    pub fn seek(&self, pos: Duration) -> Result<()> {
        self.player
            .try_seek(pos)
            .map_err(|e| anyhow!("无法跳转到指定位置：{e}"))
    }

    /// 供调试/状态栏使用：当前设备采样率等信息。
    pub fn device_config_debug(&self) -> String {
        format!("{:?}", self.sink.config())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn playable_extension_gate() {
        assert!(is_playable(Path::new("a.mp3")));
        assert!(is_playable(Path::new("a.MP3")));
        assert!(is_playable(Path::new("a.flac")));
        assert!(is_playable(Path::new("a.wav")));
        assert!(is_playable(Path::new("a.m4a")));
        assert!(is_playable(Path::new("a.ogg")));

        // Opus/WMA 能被 lofty 读标签，但 rodio 不能解码 —— 必须返回 false。
        assert!(!is_playable(Path::new("a.opus")));
        assert!(!is_playable(Path::new("a.wma")));
        assert!(!is_playable(Path::new("a.ape")));
        assert!(!is_playable(Path::new("a.txt")));
    }

    #[test]
    fn opus_is_readable_metadata_but_not_playable() {
        // 验证两个列表的差异是刻意为之，而不是笔误。
        assert!(crate::metadata::is_audio_path(Path::new("song.opus")));
        assert!(!is_playable(Path::new("song.opus")));
    }

    #[test]
    fn missing_file_is_rejected_before_touching_device() {
        // 该测试不需要真的打开音频设备：先构造一个「假引擎」不可行，
        // 因此只在没有设备时跳过，有设备时验证错误信息。
        let Ok(mut engine) = AudioEngine::new() else {
            return; // 无声卡环境（如 CI）跳过
        };
        let missing = PathBuf::from("C:/definitely/not/here.mp3");
        let err = engine.play(&missing, 0).unwrap_err();
        assert!(err.to_string().contains("文件不存在") || err.to_string().contains("不支持"));
    }

    #[test]
    fn unsupported_format_is_rejected() {
        let Ok(mut engine) = AudioEngine::new() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("echo-play-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opus = dir.join("x.opus");
        std::fs::write(&opus, b"fake").unwrap();

        let err = engine.play(&opus, 0).unwrap_err();
        assert!(err.to_string().contains("暂不支持"), "实际: {err}");
    }
}
