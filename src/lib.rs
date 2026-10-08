//! Echo —— 本地音乐播放器。
//!
//! # 分层约定
//!
//! | 层 | 模块 | 是否含平台差异 |
//! |---|---|---|
//! | 领域 | [`domain`] | 否（纯数据与纯逻辑） |
//! | 存储 | [`db`] | 否 |
//! | 元数据 | [`metadata`] | 否 |
//! | 扫描 | [`library`] | 否 |
//! | 封面 | [`covers`] | 否 |
//! | 播放 | [`playback`] | 否（rodio 内部选后端） |
//! | 界面 | [`ui`] | 仅 `ui::fonts` 需要按平台挑字体 |
//! | 平台 | [`platform`] | **是，唯一一处** |
//!
//! 新增一个平台，原则上只需在 [`platform`] 下加一个子模块；
//! 其余代码不做任何平台判定。
//!
//! 核心（domain / db / metadata / library / playback）不依赖 GUI，
//! 因此可以被集成测试单独驱动 —— 见 `tests/playback.rs`。

pub mod covers;
pub mod db;
pub mod domain;
pub mod library;
pub mod metadata;
pub mod platform;
pub mod playback;
pub mod ui;

use anyhow::{Context as _, Result};
use eframe::egui;

/// 启动桌面应用。二进制入口只是一层薄包装，方便核心逻辑被单独测试。
pub fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    prepare_graphics_backend();

    let paths = platform::paths();
    log::info!("数据目录：{}", paths.data_dir.display());

    let db = db::Db::open(&paths.db_path())
        .with_context(|| format!("无法打开数据库 {}", paths.db_path().display()))?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Echo 音乐播放器")
            .with_app_id("echo")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([820.0, 520.0]),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Echo 音乐播放器",
        options,
        Box::new(move |cc| Ok(Box::new(ui::app::EchoApp::new(cc, db)))),
    )
    .map_err(|e| anyhow::anyhow!("界面启动失败：{e}"))
}

/// 选定 wgpu 的图形后端。
///
/// **为什么需要这一步**：eframe 默认让 wgpu 按优先级自选后端，在本机的
/// Intel UHD 630 上它会选中 Vulkan，而该机的 Intel Vulkan 驱动
/// （`igvk64.dll`，31.0.101.2141）在创建交换链时崩溃：
///
/// ```text
/// 应用程序错误：echo.exe，故障模块 igvk64.dll，异常代码 0xc0000005
/// ```
///
/// 实测 DX12 与 OpenGL 后端都能稳定运行。这里在没有显式指定时优先用 DX12，
/// 避开这个驱动缺陷；一旦该驱动被修复，用户也可以自行覆盖。
///
/// 只在 Windows 上生效，且**完全尊重用户已有的设置**：
/// 若外部已经设置了 `WGPU_BACKEND`（eframe/wgpu 官方支持的环境变量），
/// 这里不做任何干预。
fn prepare_graphics_backend() {
    #[cfg(target_os = "windows")]
    {
        if std::env::var_os("WGPU_BACKEND").is_some() {
            log::debug!("已由环境变量指定 WGPU_BACKEND，跳过默认后端选择");
            return;
        }
        // SAFETY: 这段代码在 main 的最开始、任何线程被创建之前执行，
        // 因此不存在其它线程并发读取环境变量的可能。
        unsafe {
            std::env::set_var("WGPU_BACKEND", "dx12");
        }
        log::info!("默认使用 DX12 图形后端（规避本机 Intel Vulkan 驱动崩溃）");
    }
}
