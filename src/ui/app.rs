//! 界面层：`eframe::App` 实现。全部与平台无关。
//!
//! # 每帧的执行顺序（很重要）
//!
//! 1. `logic()`：消费后台事件（扫描进度、曲目播完信号、音频设备状态）；
//! 2. `ui()`：画界面，并根据用户操作改变播放状态；
//! 3. 若正在播放，请求约 100ms 后重绘，让进度条动起来。
//!
//! 之所以把「消费事件」放在画界面**之前**，是为了让同一帧里画出的
//! 状态与本帧最新的事件保持一致，避免出现一帧的闪回。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::covers::CoverCache;
use crate::db::Db;
use crate::domain::{AdvanceKind, PlayQueue, RepeatMode, ScanEvent, ScanSummary, Track};
use crate::library;
use crate::playback::{AudioEngine, is_playable};
use crate::ui::fonts;
use crate::ui::theme;
use crate::ui::widgets;
/// 状态栏提示的保留时长。
const MESSAGE_TTL: Duration = Duration::from_secs(6);

pub struct EchoApp {
    db: Db,
    /// `None` 表示没有可用音频设备（无声卡 / 远程桌面 / 容器）。
    /// 此时曲库仍可浏览，只是不能播放 —— 这是刻意的降级而非崩溃。
    audio: Option<AudioEngine>,
    covers: CoverCache,
    queue: PlayQueue,

    /// 当前搜索词，用于过滤列表。
    search: String,
    /// 按搜索词过滤后要显示的行下标。
    visible: Vec<usize>,

    /// 拖动进度条时暂存的位置（秒）。`None` 表示未在拖动。
    seek_drag: Option<f64>,

    scan_rx: Option<crossbeam_channel::Receiver<ScanEvent>>,
    scan_progress: Option<(usize, usize, String)>,
    /// 扫描目录快照，用于界面展示。
    roots: Vec<PathBuf>,

    status: Option<(String, Instant)>,
    /// 待执行的播放动作。推迟到帧尾统一处理，避免在 UI 闭包里修改播放状态。
    pending_play: Option<usize>,
}

impl EchoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, db: Db) -> Self {
        // 中文字体必须最先装：否则后面所有窗口文字都会是方块。
        fonts::install_cjk_fonts(&cc.egui_ctx);
        // 主题要紧接着装：字体大小与配色一起决定观感。
        theme::apply(&cc.egui_ctx);
        // 内嵌封面由我们自己解码（见 covers.rs），但仍安装 egui_extras 的
        // 加载器，以便将来支持从 file:// 路径加载图片。
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let audio = match AudioEngine::new() {
            Ok(engine) => Some(engine),
            Err(e) => {
                log::warn!("音频设备不可用：{e:#}");
                None
            }
        };

        let roots = db
            .scan_roots()
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let mut app = Self {
            db,
            audio,
            covers: CoverCache::new(),
            queue: PlayQueue::new(Vec::new()),
            search: String::new(),
            visible: Vec::new(),
            seek_drag: None,
            scan_rx: None,
            scan_progress: None,
            roots,
            status: None,
            pending_play: None,
        };

        if app.audio.is_none() {
            app.set_status("未找到可用音频设备，曲库可以浏览但无法播放");
        }
        app.reload_library();
        app
    }

    // ------------------------------------------------------------------
    // 状态与数据
    // ------------------------------------------------------------------

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// 从数据库重新载入曲库，并重建过滤结果。
    fn reload_library(&mut self) {
        match self.db.all_tracks_sorted() {
            Ok(tracks) => {
                self.queue.set_tracks(tracks);
                self.rebuild_visible();
            }
            Err(e) => {
                log::error!("读取曲库失败：{e:#}");
                self.set_status(format!("读取曲库失败：{e}"));
            }
        }
    }

    /// 按搜索词重算可见行。
    fn rebuild_visible(&mut self) {
        let needle = self.search.trim().to_lowercase();
        self.visible = self
            .queue
            .tracks()
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                if needle.is_empty() {
                    return true;
                }
                // 标题、艺术家、专辑、文件名都参与匹配，方便按任意信息找歌。
                let hay = format!(
                    "{} {} {} {}",
                    t.title,
                    t.artist,
                    t.album,
                    t.path
                        .file_name()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default()
                )
                .to_lowercase();
                hay.contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
    }

    /// 当前曲目在队列中的下标。
    fn current_index(&self) -> Option<usize> {
        self.queue.cursor()
    }

    fn current_track(&self) -> Option<&Track> {
        self.queue.current()
    }

    /// 开始播放队列中的某一首。
    fn play_index(&mut self, index: usize) {
        let Some(track) = self.queue.get(index) else {
            return;
        };
        let path: PathBuf = track.path.clone();
        let title = track.display_title();

        let Some(audio) = self.audio.as_mut() else {
            self.set_status("未找到可用音频设备，无法播放");
            return;
        };

        // 无论能否解码，都先把光标移过去：这样界面会显示「当前曲目」，
        // 用户能看清是哪一首出了问题。
        self.queue.jump_to(index);
        self.seek_drag = None;

        match audio.play(&path, index) {
            Ok(()) => {
                log::info!("开始播放：{}", path.display());
                let _ = title;
            }
            Err(e) => {
                log::warn!("播放失败 {}：{e:#}", path.display());
                self.set_status(format!("播放失败：{e}"));
            }
        }
    }

    /// 切歌。`kind` 决定单曲循环是否生效。
    fn advance(&mut self, kind: AdvanceKind) {
        if self.queue.is_empty() {
            return;
        }
        let target = self.queue.peek_next(kind);
        match target {
            Some(i) => self.play_index(i),
            None => {
                // 顺序播放到头：停止并复位到第一首，等待用户操作。
                if let Some(audio) = self.audio.as_ref() {
                    audio.stop();
                }
                self.queue.jump_to(0);
                self.set_status("已播放到列表末尾");
            }
        }
    }

    fn advance_prev(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        match self.queue.peek_prev(AdvanceKind::Manual) {
            Some(i) => self.play_index(i),
            None => self.set_status("已经是第一首"),
        }
    }

    /// 添加音乐文件夹。
    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("选择音乐文件夹");
        // 让对话框从已有的扫描目录开始，减少来回点击。
        if let Some(first) = self.roots.first().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(first);
        }
        if let Some(dir) = dialog.pick_folder() {
            if let Err(e) = self.db.add_scan_root(&dir) {
                self.set_status(format!("保存目录失败：{e}"));
                return;
            }
            self.roots.push(dir.clone());
            self.set_status(format!("已添加：{}", dir.display()));
            self.start_scan();
        }
    }

    /// 启动一次后台扫描。
    fn start_scan(&mut self) {
        if self.roots.is_empty() {
            self.set_status("还没有添加任何音乐文件夹，请先点击「添加文件夹」");
            return;
        }
        if self.scan_rx.is_some() {
            self.set_status("扫描正在进行中…");
            return;
        }
        let db = self.db.clone();
        let rx = library::start_scan(self.roots.clone(), db);
        self.scan_rx = Some(rx);
        self.scan_progress = Some((0, 0, String::new()));
        self.set_status("开始扫描…");
    }

    /// 消费后台事件。返回是否需要重绘。
    fn drain_scan_events(&mut self) {
        // 先把消息取出来，再处理，避免持有 rx 的同时借 &mut self。
        let mut events = Vec::new();
        if let Some(rx) = self.scan_rx.as_ref() {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }

        let mut finished = false;
        for ev in events {
            match ev {
                ScanEvent::Started { total } => {
                    self.scan_progress = Some((0, total, String::new()));
                }
                ScanEvent::Progress {
                    done,
                    total,
                    current,
                } => {
                    self.scan_progress = Some((done, total, current));
                }
                ScanEvent::Finished(summary) => {
                    self.on_scan_finished(&summary);
                    finished = true;
                }
                ScanEvent::Failed(err) => {
                    self.set_status(format!("扫描失败：{err}"));
                    finished = true;
                }
            }
        }
        if finished {
            self.scan_rx = None;
            self.scan_progress = None;
        }
    }

    fn on_scan_finished(&mut self, summary: &ScanSummary) {
        log::info!("扫描完成：{summary:?}");
        self.set_status(format!("扫描完成：{}", summary.describe()));
        // 元数据可能变了，封面必须重解码。
        self.covers.clear();
        self.reload_library();
    }

    /// 处理「上一曲播完了」。
    fn handle_finished_track(&mut self) {
        let Some(finished) = self.audio.as_ref().and_then(|a| a.take_finished_index()) else {
            return;
        };
        // 只有当播完的确实是当前曲目时才自动切歌，避免旧回调误触发。
        if Some(finished) != self.current_index() {
            log::debug!("忽略过期的播完信号：{finished}");
            return;
        }
        let repeat = self.queue.repeat();
        match repeat {
            // 顺序播放且已是最后一首：停下。
            RepeatMode::Off if self.queue.peek_next(AdvanceKind::Auto).is_none() => {
                if let Some(audio) = self.audio.as_ref() {
                    audio.stop();
                }
                self.set_status("已播放到列表末尾");
            }
            _ => self.advance(AdvanceKind::Auto),
        }
    }

    /// 处理本帧累积的播放请求。
    fn flush_pending(&mut self) {
        if let Some(i) = self.pending_play.take() {
            self.play_index(i);
        }
    }

    // ------------------------------------------------------------------
    // 界面各区域
    // ------------------------------------------------------------------

    fn library_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("添加文件夹").clicked() {
                self.pick_folder();
            }
            if ui.button("重新扫描").clicked() {
                self.start_scan();
            }
        });

        ui.horizontal(|ui| {
            ui.label("搜索");
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("标题 / 艺术家 / 专辑")
                        .desired_width(f32::INFINITY),
                )
                .changed();
            if changed {
                self.rebuild_visible();
            }
        });

        if !self.roots.is_empty() {
            ui.collapsing("已扫描的目录", |ui| {
                for r in &self.roots {
                    ui.small(widgets::ellipsize(&r.display().to_string(), 40));
                }
            });
        }

        ui.separator();

        if self.queue.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label("曲库为空");
                ui.small("点击上面的「添加文件夹」开始建立曲库");
            });
            return;
        }

        if self.visible.is_empty() {
            ui.label("没有匹配的曲目");
            return;
        }

        let current = self.current_index();
        // 先把要用到的数据取出来，避免在闭包里同时借用 self 的多个字段。
        let rows: Vec<(usize, String, String, String, Option<String>, bool)> = self
            .visible
            .iter()
            .filter_map(|&i| {
                let t = self.queue.get(i)?;
                Some((
                    i,
                    t.display_title(),
                    t.display_artist().to_owned(),
                    t.duration_text(),
                    t.cover_hash.clone(),
                    is_playable(&t.path),
                ))
            })
            .collect();

        let mut clicked: Option<usize> = None;

        // 取出封面缓存与上下文，让闭包只借用它们而不借用整个 self。
        let ctx = ui.ctx().clone();
        let mut covers = std::mem::take(&mut self.covers);
        let db = self.db.clone();

        // 虚拟化渲染：只实例化可见区间的行。
        //
        // 之前这里是全量 `for` 循环，几千首曲目会一次性构建全部行、
        // 解码全部缩略图，界面会明显卡顿。show_rows 由 egui 算好可见区间，
        // 我们只画那几十行 —— 这是行高必须固定的原因。
        let row_height = theme::ROW_HEIGHT + ui.spacing().item_spacing.y;
        let total = rows.len();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, total, |ui, range| {
                for i in range {
                    let Some((index, title, artist, duration, cover_hash, playable)) = rows.get(i)
                    else {
                        continue;
                    };
                    let thumbnail = covers.get_for_track(&ctx, &db, cover_hash.as_deref());
                    let row = widgets::TrackRow {
                        index: *index,
                        title,
                        artist,
                        duration,
                        is_current: current == Some(*index),
                        thumbnail: thumbnail.as_ref(),
                        playable: *playable,
                    };
                    if widgets::track_row(ui, &row) {
                        clicked = Some(*index);
                    }
                    // 用留白而不是分隔线：靠背景层次区分行，观感更干净。
                    ui.add_space(ui.spacing().item_spacing.y);
                }
            });

        self.covers = covers;
        if let Some(i) = clicked {
            self.pending_play = Some(i);
        }
    }

    /// 画底部播放控制条。
    ///
    /// 返回本帧检测到的按钮点击，而**不**在这里改播放状态：
    /// egui 的闭包会借用 `self`，若在闭包内调用需要 `&mut self` 的方法会冲突。
    /// 把动作收集出来、回到 `ui()` 之后再执行，是最干净的写法。
    fn player_panel(&mut self, ui: &mut egui::Ui) -> PlayerActions {
        let mut actions = PlayerActions::default();

        let has_device = self.audio.is_some();
        let Some(track_index) = self.current_index() else {
            ui.label("未选择曲目");
            return actions;
        };
        let Some(track) = self.queue.get(track_index).cloned() else {
            ui.label("未选择曲目");
            return actions;
        };

        let is_paused = self.audio.as_ref().map(|a| a.is_paused()).unwrap_or(true);
        let total = Duration::from_millis(track.duration_ms);
        let live_pos = self
            .audio
            .as_ref()
            .map(|a| a.position())
            .unwrap_or(Duration::ZERO);
        let any_playing = self.audio.as_ref().map(|a| !a.is_idle()).unwrap_or(false);

        // ---- 第一行：曲目信息 ----
        ui.horizontal(|ui| {
            ui.strong(track.display_title());
            ui.label("—");
            ui.label(track.display_artist());
            if !track.album.is_empty() {
                ui.weak(format!("《{}》", track.album));
            }
        });

        // ---- 第二行：传输控制与进度 ----
        ui.horizontal(|ui| {
            if ui
                .add_enabled(has_device, egui::Button::new("⏮ 上一首"))
                .clicked()
            {
                actions.prev = true;
            }
            let toggle_label = if is_paused {
                "▶ 播放"
            } else {
                "⏸ 暂停"
            };
            if ui
                .add_enabled(has_device, egui::Button::new(toggle_label))
                .clicked()
            {
                actions.toggle = true;
            }
            if ui
                .add_enabled(has_device, egui::Button::new("下一首 ⏭"))
                .clicked()
            {
                actions.next = true;
            }

            let repeat = self.queue.repeat();
            if ui
                .add(egui::Button::new(format!(
                    "{} {}",
                    repeat.icon(),
                    repeat.label()
                )))
                .on_hover_text("切换循环模式")
                .clicked()
            {
                actions.cycle_repeat = true;
            }

            ui.separator();

            // ---- 进度条 ----
            let total_secs = total.as_secs_f64().max(0.0);
            let mut pos_secs = self.seek_drag.unwrap_or(live_pos.as_secs_f64());
            if total_secs > 0.0 {
                let slider = ui.add_enabled(
                    has_device,
                    egui::Slider::new(&mut pos_secs, 0.0..=total_secs)
                        .show_value(false)
                        .trailing_fill(true),
                );
                // 拖动过程中只更新暂存值，松手才真正 seek ——
                // 否则每一帧都会触发一次跳转，听起来会卡顿甚至爆音。
                if slider.dragged() {
                    actions.seek_drag = Some(pos_secs);
                }
                if slider.drag_stopped() {
                    actions.commit_seek = Some(pos_secs);
                }
            } else {
                // 时长未知（部分文件没写头信息）：退化成不确定进度条。
                ui.add(egui::ProgressBar::new(0.0).desired_width(120.0));
            }

            let shown = self.seek_drag.unwrap_or(live_pos.as_secs_f64());
            ui.small(format!(
                "{} / {}",
                crate::domain::format_duration(widgets::secs_to_duration(shown)),
                crate::domain::format_duration(total)
            ));
        });

        // ---- 第三行：音量 ----
        ui.horizontal(|ui| {
            ui.label("🔊");
            let mut vol = self.audio.as_ref().map(|a| a.volume()).unwrap_or(0.0);
            let resp = ui
                .scope(|ui| {
                    // egui 0.36 的 Slider 没有宽度方法，宽度来自样式里的
                    // spacing.slider_width；用 scope 临时改，避免影响别处。
                    ui.spacing_mut().slider_width = 120.0;
                    ui.add_enabled(
                        has_device,
                        egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false),
                    )
                })
                .inner;
            if resp.changed() {
                actions.volume = Some(vol);
            }
            ui.small(format!("{:.0}%", vol * 100.0));

            if any_playing {
                ui.separator();
                ui.small("正在播放");
            }
        });

        actions
    }

    fn now_playing_panel(&mut self, ui: &mut egui::Ui) {
        let Some(track) = self.current_track().cloned() else {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading("Echo");
                ui.label("从左侧曲库中选择一首开始播放");
                if self.audio.is_none() {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 140, 60),
                        "提示：当前没有可用音频设备",
                    );
                }
            });
            return;
        };

        // 双栏：左侧封面、右侧信息。居中的单栏在这块宽画布上会显得空。
        let ctx2 = ui.ctx().clone();
        let mut covers = std::mem::take(&mut self.covers);
        let db2 = self.db.clone();
        let thumb = covers.get_for_track(&ctx2, &db2, track.cover_hash.as_deref());
        self.covers = covers;

        let cover_side = 300.0_f32.min(ui.available_width() * 0.42);
        let row_height = cover_side.max(220.0);

        ui.add_space(24.0);
        ui.horizontal_top(|ui| {
            // 让整块内容在水平方向大致居中。
            let indent = ((ui.available_width() - (cover_side + 340.0)) * 0.5).max(0.0);
            ui.add_space(indent);

            widgets::big_cover(ui, thumb.as_ref(), cover_side);
            ui.add_space(36.0);

            ui.allocate_ui_with_layout(
                egui::Vec2::new(300.0, row_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // 让文字块与封面垂直居中。
                    ui.add_space((row_height - 210.0).max(0.0) * 0.5);

                    ui.label(
                        egui::RichText::new(track.display_title())
                            .size(26.0)
                            .color(theme::TEXT),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(track.display_artist())
                            .size(16.0)
                            .color(theme::TEXT_DIM),
                    );
                    if !track.album.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("《{}》", track.album))
                                .size(14.0)
                                .color(theme::TEXT_FAINT),
                        );
                    }

                    ui.add_space(18.0);

                    // 元信息排成一行「胶囊」，比一串用 · 连接的灰字更清楚。
                    let mut chips: Vec<String> = vec![track.duration_text()];
                    if let Some(year) = track.year {
                        chips.push(format!("{year} 年"));
                    }
                    if !track.genre.is_empty() {
                        chips.push(track.genre.clone());
                    }
                    if let Some(rate) = track.sample_rate {
                        chips.push(format!("{:.1} kHz", rate as f32 / 1000.0));
                    }
                    ui.horizontal_wrapped(|ui| {
                        for chip in &chips {
                            chip_label(ui, chip);
                        }
                    });

                    if !is_playable(&track.path) {
                        ui.add_space(12.0);
                        ui.colored_label(
                            egui::Color32::from_rgb(0xe8, 0x95, 0x54),
                            "⚠ 该格式 rodio 无法解码，仅能展示元数据",
                        );
                    }

                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new(widgets::ellipsize(
                            &track.path.display().to_string(),
                            56,
                        ))
                        .small()
                        .color(theme::TEXT_FAINT),
                    );
                },
            );
        });
    }
}

/// 小而圆的标签，用于展示时长/年份/流派这类元信息。
fn chip_label(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::BG_PANEL)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).small().color(theme::TEXT_DIM));
        });
}

/// 传输区本帧检测到的用户操作。
///
/// 用一个小结构体把「UI 里检测到的点击」与「对播放状态的修改」解耦：
/// egui 的闭包会借用 `self`，在闭包内调用需要 `&mut self` 的方法会冲突。
/// 先收集动作、回到 `ui()` 之后再执行，是最干净的写法。
#[derive(Clone, Copy, Default)]
struct PlayerActions {
    prev: bool,
    toggle: bool,
    next: bool,
    cycle_repeat: bool,
    /// 拖动中：只更新暂存位置，不动音频。
    seek_drag: Option<f64>,
    /// 拖动结束：真正执行跳转。
    commit_seek: Option<f64>,
    volume: Option<f32>,
}

/// 执行传输区收集到的动作。
impl EchoApp {
    fn apply_player_actions(&mut self, actions: PlayerActions) {
        if let Some(vol) = actions.volume
            && let Some(audio) = self.audio.as_mut()
        {
            audio.set_volume(vol);
        }
        if let Some(pos) = actions.seek_drag {
            self.seek_drag = Some(pos);
        }
        if let Some(pos) = actions.commit_seek {
            if let Some(audio) = self.audio.as_ref()
                && let Err(e) = audio.seek(widgets::secs_to_duration(pos))
            {
                self.set_status(format!("跳转失败：{e}"));
            }
            self.seek_drag = None;
        }
        if actions.cycle_repeat {
            let new_mode = self.queue.repeat().next_mode();
            self.queue.set_repeat(new_mode);
            self.set_status(new_mode.label());
        }
        if actions.toggle
            && let Some(audio) = self.audio.as_ref()
        {
            audio.toggle();
        }
        if actions.prev {
            self.advance_prev();
        }
        if actions.next {
            self.advance(AdvanceKind::Manual);
        }
    }
}

impl eframe::App for EchoApp {
    /// 每帧的非绘制逻辑：消费后台事件。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_scan_events();
        self.handle_finished_track();

        // 提示信息到期自动消失。
        if let Some((_, at)) = &self.status
            && at.elapsed() > MESSAGE_TTL
        {
            self.status = None;
        }

        // 正在播放时定期重绘，让进度条持续刷新。
        let playing = self
            .audio
            .as_ref()
            .map(|a| !a.is_idle() && !a.is_paused())
            .unwrap_or(false);
        if playing || self.scan_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 顶部状态条
        egui::Panel::top("status").show(ui, |ui| {
            let progress = self.scan_progress.as_ref().map(|(done, total, current)| {
                widgets::ScanProgressView {
                    done: *done,
                    total: *total,
                    current: current.clone(),
                }
            });
            let bar = widgets::StatusBar {
                scanning: progress.as_ref(),
                message: self.status.as_ref().map(|(m, _)| m.as_str()),
                library_count: self.queue.len(),
            };
            widgets::status_bar(ui, &bar);
        });

        // 左侧曲库
        egui::Panel::left("library")
            .resizable(true)
            .default_size(320.0)
            .min_size(220.0)
            .show(ui, |ui| {
                self.library_panel(ui);
            });

        // 底部播放控制
        let mut actions = PlayerActions::default();
        egui::Panel::bottom("player")
            .resizable(false)
            .show(ui, |ui| {
                actions = self.player_panel(ui);
            });

        // 中间正在播放
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.now_playing_panel(ui);
            });
        });

        // 本帧累积的用户操作统一在这里执行，避免在 egui 闭包里改播放状态。
        self.apply_player_actions(actions);
        self.flush_pending();
    }
}
