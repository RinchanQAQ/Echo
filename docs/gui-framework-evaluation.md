# GUI 框架替换可行性评估：iced vs Slint

> 面向项目：`echo` 本地音乐播放器（Rust, windows-msvc, 无 Visual Studio，rust-lld + clang-cl）
> 现状：`eframe/egui = "=0.36.2"`，UI 已在 `src/ui/` 内与音频/数据库/元数据解耦。
>
> **本文所有结论均来自实测抓取的在线来源**（crates.io API、docs.rs、docs.slint.dev、GitHub raw 源码）。
> 凡未能核实的点，统一列在各节「无法核实的点」中。除特别说明，URL 均为本次实际抓取成功的地址。

---

## 结论速览（先看这个）

| 维度 | iced 0.14 | Slint 1.18 |
|---|---|---|
| 最新稳定版 | **0.14.0**（crates.io `max_stable_version`） | **1.18.1** |
| 许可证 | **MIT**（无附加条件） | GPL-3.0-only **OR** Royalty-free-2.0 **OR** 商业许可（三选一） |
| 商业闭源免费？ | 是，无条件 | 是，**但必须**显示 `AboutSlint` 或署名徽章；嵌入式需付费 |
| 默认 GPU 后端 | **wgpu 27**（默认含 Vulkan！）→ tiny-skia 软件回退 | **FemtoVG + OpenGL**（默认不含 Vulkan）→ 软件回退 |
| 你的 Intel Vulkan 崩溃风险 | **高**（`Backends::PRIMARY` 含 Vulkan），可修：`WGPU_BACKEND=dx12` | **无**（Skia-Vulkan 是 opt-in feature） |
| 列表虚拟化 | 有（`widget::lazy`，需开 `lazy` feature） | 有（`ListView`，文档明说只实例化可见项） |
| 内存 RGBA → 图片 | 有，`image::Handle::from_rgba` | 有，`Image::from_rgba8(SharedPixelBuffer)` |
| PNG 字节 → 图片 | 有，`image::Handle::from_bytes` | 有，`Image::load_from_data(&[u8], None)` |
| 滑条 | 有，`widget::slider` | 有，`Slider` 标准控件 |
| 中文字体 | 运行期字节流，**`.ttc` 支持已从 fontdb 源码证实** | 编译期 `import "x.ttc"`，**官方文档明说支持 `.ttc`**；运行期 API 仍是 unstable |
| 换主题/深色 | 22 个内置配色 + `Theme::custom` | `Palette.color-scheme: dark`；6 种控件样式（fluent/material/cupertino/cosmic/qt/native） |
| 圆角/阴影/卡片 | 有（`container::Style` + `Shadow`/`Border::radius`） | 有（`border-radius` / `drop-shadow-*` / `inner-shadow-*`） |
| 默认观感 | 素；**内置的是配色，不是控件设计** | 开箱即用的 Fluent 风格控件 |
| 官方现代外观示例 | examples 目录 + awesome-iced（第三方） | `gallery` 等 20+ 官方示例，含 wasm 在线 demo |

**一句话结论**：想「好看」——**Slint 更快**（默认就是 Fluent 设计系统 + 开箱控件），但代价是引入 `.slint` DSL、`build.rs` 代码生成、署名义务、编译期字体。
想「可控」——**iced 更稳**（MIT、纯 Rust、DX12 强制开关、运行期字体），但**默认丑**，好看与否 100% 取决于你自己写多少样式代码，且必须改掉 Vulkan 默认后端。

---

# A. iced

## A1. 版本号 + 许可证

| 项 | 值 | 来源 |
|---|---|---|
| 最新稳定版 | `0.14.0` | [crates.io API](https://crates.io/api/v1/crates/iced)（`max_stable_version: "0.14.0"`） |
| 许可证 | **MIT** | 同上，`"license":"MIT"` |
| MSRV | `rust_version = "1.88"` | `Cargo.toml` workspace，[raw 源码](https://raw.githubusercontent.com/iced-rs/iced/0.14/Cargo.toml) |
| edition | 2024 | 同上 |
| 仓库 path 版本 | `0.14.1`（发布分支已到 0.14.1，crates.io 上最新是 0.14.0） | 同上，`[workspace.package] version = "0.14.1"` |

> 注意：仓库 `0.14` 分支的 workspace 版本是 **0.14.1**，但 crates.io 上 `max_stable_version` 仍是 **0.14.0**。落地时用 `iced = "0.14"` 即可，Cargo 会解析到已发布的最新 patch。

**默认 feature 集**（`Cargo.toml` 原文）：
```toml
default = ["wgpu", "tiny-skia", "crisp", "web-colors", "thread-pool",
           "linux-theme-detection", "x11", "wayland"]
```
注意 `default` **不含** `lazy`（虚拟列表要自己开）、**不含** `image`（图片控件要自己开）、**不含** `advanced`。
`default` **同时含** `wgpu` 和 `tiny-skia` —— 这两者会组合出「wgpu 优先、失败自动降级到软件渲染」的行为（见 A2）。

## A2. 渲染后端与 Windows 上的 Vulkan 风险

### 后端事实

iced 用的是 **wgpu**，不是自研后端。`iced_renderer` 在 `wgpu-bare` 特性下直接 `pub use iced_wgpu as wgpu`：

```rust
// renderer/src/lib.rs
#[cfg(feature = "wgpu-bare")]
pub use iced_wgpu as wgpu;
```
来源：[renderer/src/lib.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/lib.rs)

wgpu 版本：workspace `wgpu = { version = "27.0", ... }`
来源：[Cargo.toml](https://raw.githubusercontent.com/iced-rs/iced/0.14/Cargo.toml)

### 关键风险（对你的机器）

**默认会走 Vulkan。** `iced_wgpu` 的 headless 路径写得很清楚：

```rust
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
    ...
});
```
来源：[wgpu/src/lib.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/lib.rs)

而 `Backends::PRIMARY` 的定义（docs.rs wgpu）是：

> `VULKAN`, `METAL`, `DX12`, `BROWSER_WEBGPU`

来源：[docs.rs/wgpu/…/struct.Backends.html](https://docs.rs/wgpu/latest/wgpu/struct.Backends.html)

也就是说：**在你没设环境变量时，Vulkan 在候选集中，而 wgpu 的 `power_preference` 默认是 `HighPerformance`**（`wgpu/src/window/compositor.rs` 里 `wgpu::PowerPreference::from_env().unwrap_or(HighPerformance)`），在高性能独显/核显上极可能优先选中 Vulkan → **直接命中你机器上 igvk64.dll 崩溃的问题**。

### 强制 DX12 / 软件渲染的三种办法（都已核实）

1. **`WGPU_BACKEND` 环境变量**（最省事，wgpu 官方支持）
   wgpu 文档原文：
   > `pub fn from_env() -> Option<Backends>`
   > Gets a set of backends from the environment variable **`WGPU_BACKEND`**.
   > See `Self::from_comma_list()` for the format of the string.
   >
   > Names: vulkan = "vulkan"/"vk"; **dx12 = "dx12"/"d3d12"**; metal = "metal"/"mtl"; gles = "opengl"/"gles"/"gl"; webgpu = "webgpu"

   来源：[docs.rs/wgpu/…/struct.Backends.html](https://docs.rs/wgpu/latest/wgpu/struct.Backends.html)

   同时 `iced_wgpu` 的 compositor **显式读取**它：
   ```rust
   if let Some(backends) = wgpu::Backends::from_env() {
       settings.backends = backends;
   }
   ```
   来源：[wgpu/src/window/compositor.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/window/compositor.rs)

   → **`set WGPU_BACKEND=dx12` 就是你要的那个开关**（写进 `main.rs` 顶部用 `std::env::set_var` 也行，但必须在任何 `Application::run()` 之前）。

2. **`ICED_BACKEND` 环境变量**（选择 iced 自己的渲染器，不是图形 API）
   `iced_renderer::fallback::Compositor::with_backend` 里：
   ```rust
   let backends = backend
       .map(str::to_owned)
       .or_else(|| env::var("ICED_BACKEND").ok());
   // 支持逗号分隔，如 ICED_BACKEND="wgpu,tiny-skia"
   ```
   来源：[renderer/src/fallback.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/fallback.rs)

   ⚠️ **注意别搞混**：`ICED_BACKEND` 选的是 `wgpu` / `tiny-skia` 这一层（软件渲染器）、`WGPU_BACKEND` 选的是 Vulkan/DX12/GL 这一层。**要避开 Intel Vulkan，你要的是 `WGPU_BACKEND=dx12`。**

3. **`ICED_PRESENT_MODE`**：控制 vsync/present mode（`vsync`/`no_vsync`/`immediate`/`fifo`/`fifo_relaxed`/`mailbox`）
   来源：[wgpu/src/settings.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/settings.rs)

### 软件渲染回退是真的存在（这是你的一张安全网）

`iced_renderer::Renderer` 的类型别名在 `wgpu-bare` + `tiny-skia` 同时开启时（= 默认配置）是：

```rust
pub type Renderer = crate::fallback::Renderer<iced_wgpu::Renderer, iced_tiny_skia::Renderer>;
pub type Compositor = crate::fallback::Compositor<
    iced_wgpu::window::Compositor, iced_tiny_skia::window::Compositor>;
```
来源：[renderer/src/lib.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/lib.rs)

而 `Compositor::with_backend` 的实现是逐候选**依次尝试 A 再尝试 B**：
```rust
match A::with_backend(...).await {
    Ok(compositor) => return Ok(Self::Primary(compositor)),
    Err(error) => { errors.push(error); }
}
match B::with_backend(...).await {
    Ok(compositor) => return Ok(Self::Secondary(compositor)),
    Err(error) => { errors.push(error); }
}
```
来源：[renderer/src/fallback.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/fallback.rs)

**但要注意**：这个回退的触发条件是「wgpu 初始化失败（返回 `Err`）」。Intel Vulkan 驱动**崩溃**（进程级 abort）不是 `Err`，回退救不了你。所以「显式 `WGPU_BACKEND=dx12`」比「指望回退」可靠得多。

### 无法核实的点
- 我**没有**核实到 iced 官方有「程序内直接设置 `backends` 字段」的公开 API。`iced_wgpu::Settings::backends` 字段是 `pub` 的，但 `iced::Application` 只暴露 `settings(iced::Settings)`，而 **`iced::Settings` 的字段只有 `id / fonts / default_font / default_text_size / antialiasing / vsync`**——**没有 backends 字段**。来源：[docs.rs/iced/0.14.0/iced/struct.Settings.html](https://docs.rs/iced/0.14.0/iced/struct.Settings.html)、[core/src/settings.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/settings.rs)。**结论：走环境变量是你唯一已核实的路径。**
- 我**没有**在 iced 仓库检索到针对 `igvk64.dll` / Intel Vulkan 的已知 issue（搜索命中大多是 DLL 下载站，非技术来源）。也就是说：**这不是 iced 的已知 bug，是 wgpu 默认后端选择与你的驱动环境冲突**。

## A3. 能力 1–6 逐条

### 1) 虚拟化长列表 —— ✅ 支持（需开 `lazy` feature）

- **`iced::widget::lazy`**，签名（真实源码）：
  ```rust
  pub fn lazy<'a, Message, Theme, Renderer, Dependency, View>(
      dependency: Dependency,
      view: impl Fn(&Dependency) -> View + 'a,
  ) -> Lazy<'a, Message, Theme, Renderer, Dependency, View>
  where
      Dependency: Hash + 'a,
      View: Into<Element<'static, Message, Theme, Renderer>>,
  ```
  来源：[widget/src/lazy/helpers.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lazy/helpers.rs)
  触发重算的机制是 `Dependency: Hash`——`diff` 时哈希不变就**复用上一帧的 widget 树**：
  ```rust
  if current.hash != new_hash { /* 重建 */ } else { /* 复用 */ }
  ```
  来源：[widget/src/lazy.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lazy.rs)

- ⚠️ **重要提醒**：`widget::lazy` / `scrollable` **本身不提供「只渲染可见行」的真虚拟化**。它提供的是「依赖没变就不重建子树」。要真正只构建可见行，你得自己把「当前滚动偏移 → 可见索引区间」算出来，把那一段做成 `Dependency`，再交给 `lazy`。**iced 0.14 没有开箱的 `VirtualList`/`List` 控件。**
- 另外 iced 0.14 新增了 **`iced::widget::table`**（`widget/src/table.rs` 存在，见 [widget/src/lib.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lib.rs) 的 `pub mod table;`）。但看实现，`Table::new` 会**为所有行构建 cell**：
  ```rust
  for row in rows { for view in &views { let cell = view(row.clone()); ... cells.push(cell); } }
  ```
  来源：[widget/src/table.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/table.rs)
  → **`table` 不是虚拟化的**，几千首歌别用它当曲库列表。
- 官方示例 `examples/lazy`（若存在）与 `examples/scrollable` 可参考：[examples 目录](https://github.com/iced-rs/iced/tree/0.14/examples)

**给音乐播放器的实操建议**：`scrollable` + 自己算可见区间 + `lazy(visible_range, ...)`。工作量中等。

### 2) 从内存 RGBA/PNG 显示图片 —— ✅ 支持（需开 `image` feature）

`iced::widget::image::Handle` 的三个构造器（真实源码）：
```rust
pub fn from_path<T: Into<PathBuf>>(path: T) -> Handle;              // 文件
pub fn from_bytes(bytes: impl Into<Bytes>) -> Handle;                // 编码字节（PNG/JPG…）
pub fn from_rgba(width: u32, height: u32, pixels: impl Into<Bytes>) -> Handle;  // 已解码 RGBA
```
来源：[core/src/image.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/image.rs)

用法：
```rust
use iced::widget::image;
image(iced::widget::image::Handle::from_bytes(png_vec))
// 或
image(iced::widget::image::Handle::from_rgba(w, h, rgba_vec))
```
控件还支持 `Image::border_radius(...)`、`content_fit(...)`、`opacity(...)`、`crop(rectangle)`、`filter_method(...)`、`rotation(...)`、`scale(...)`
来源同上 + [core/src/image.rs `Image` struct](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/image.rs)

- 文档页：[docs.rs/iced/0.14.0/iced/widget/image/fn.image.html](https://docs.rs/iced/0.14.0/iced/widget/image/fn.image.html)、[enum.Handle.html](https://docs.rs/iced/0.14.0/iced/widget/image/enum.Handle.html)
- `image` feature 会拉入 `image` crate 全部默认解码器；若你只想用已有的 `image = 0.25.10`，用 `image-without-codecs` 更省。特性定义见 [Cargo.toml](https://raw.githubusercontent.com/iced-rs/iced/0.14/Cargo.toml)：`image = ["image-without-codecs", "image/default"]`。
- ⚠️ 性能注意：`Handle::from_rgba` 每次是**新 `Id::unique()`**，意味着渲染器会当成新图片重新上传 GPU。**每帧新建 handle 会持续吃显存/带宽**，请缓存 handle（这是源码里 `Id::unique()` 的必然推论，非文档明说）。

### 3) 可拖动滑条 —— ✅ 支持

`iced::widget::slider`，构造：
```rust
pub fn new<F>(range: RangeInclusive<T>, value: T, on_change: F) -> Self
```
实用方法：`.step(...)`、`.shift_step(...)`（按住 Shift 用大步长）、`.default(...)`（Ctrl+点击复位）、`.on_release(msg)`（**拖完才发消息，正好适合 seek 而非每像素一次 seek**）、`.height(...)`、`.style(...)`
来源：[widget/src/slider.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/slider.rs)

样式（0.14 是 Catalog 模式，不是 0.12 的 StyleSheet）：
```rust
pub struct Style { pub rail: Rail, pub handle: Handle }
impl Style { pub fn with_circular_handle(self, radius: impl Into<Pixels>) -> Self }
pub struct Rail { pub backgrounds: (Background, Background), pub width: f32, pub border: Border }
pub struct HandleShape { Circle { radius: f32 }, Rectangle { width: u16, border_radius: border::Radius } }
pub trait Catalog { type Class<'a>; fn default<'a>() -> Self::Class<'a>; fn style(&self, class: &Self::Class<'_>, status: Status) -> Style; }
```
来源同上。交互上已内建：拖拽、方向键、Ctrl+滚轮、hover/drag 三态。
→ **进度条/音量条开箱可用，且 `on_release` 正好解决 seek 抖动问题。**

### 4) 中文字体（含 .ttc）—— ✅ 支持，且**与你的现有代码天然对接**

加载 API：
```rust
// iced::application(...)
pub fn font(mut self, font: impl Into<Cow<'static, [u8]>>) -> Self  // 可多次调用
```
来源：[src/application.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/src/application.rs)、[docs.rs Application](https://docs.rs/iced/0.14.0/iced/application/struct.Application.html)

底层落到 `iced_graphics::text::FontSystem::load_font`：
```rust
pub fn load_font(&mut self, bytes: Cow<'static, [u8]>) {
    ...
    let _ = self.raw.db_mut().load_font_source(
        cosmic_text::fontdb::Source::Binary(Arc::new(bytes.into_owned())),
    );
    self.version = Version(self.version.0 + 1);
}
```
来源：[graphics/src/text.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/graphics/src/text.rs)

选定字族：`iced::Font::with_name("Microsoft YaHei")` / `Font { family: font::Family::Name("..."), weight, stretch, style }`
来源：[core/src/font.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/font.rs)

**`.ttc` 字体集合：支持，已从 fontdb 源码证实**（iced 依赖 `cosmic-text = "0.15"`，其内部用 `fontdb`）：
```rust
/// Loads a font data into the `Database`.
/// Will load all font faces in case of a font collection.
pub fn load_font_data(&mut self, data: Vec<u8>) { ... }

pub fn load_font_source(&mut self, source: Source) -> TinyVec<[ID; 8]> {
    let ids = source.with_data(|data| {
        let n = ttf_parser::fonts_in_collection(data).unwrap_or(1);   // ← .ttc 多 face
        for index in 0..n { ... }
    });
    ...
}
```
来源：[fontdb/src/lib.rs](https://raw.githubusercontent.com/RazrFalcon/fontdb/master/src/lib.rs)（`ttf-parser::fonts_in_collection` + 逐 face 装载）

同一份源码的 `load_fonts_dir` 也明确列出扩展名白名单：`ttf | ttc | otf | otc`（含大写），且 Windows 分支会扫 `%SYSTEMROOT%\Fonts`。

**→ 对你项目的意义（重要）**：你 `src/platform/windows.rs` 的 `cjk_font_candidates()` 第一项就是 `msyh.ttc`，而 `platform::load_first_available_font()` 返回的是 `LoadedFont { bytes: Vec<u8>, label }`（见 [src/platform/mod.rs](D:\Echo\src\platform\mod.rs)）。所以你可以**直接复用现有代码**：

```rust
let mut app = iced::application(App::new, App::update, App::view).font(bytes_from_existing_loader);
// 或显式指定字族
app = app.default_font(iced::Font::with_name("Microsoft YaHei"));
```

⚠️ **一个已核实的行为差异**：egui 里你是把 CJK 字体**追加到族列表末尾**当回退（拉丁仍用 Ubuntu-Light）。iced 没有「追加到族末尾」的概念——`.font(bytes)` 只是**注册**字体，实际用哪个族由 `Font`/`Family` 决定。想保留「拉丁用 Fira、汉字回退到雅黑」的效果，你要：
1. 开 `fira-sans` feature（或自己 `.font(拉丁字体字节)`）；2. 设 `default_font` 为拉丁族；3. 依赖 cosmic-text 的字体回退自动补汉字。
   **`iced::Settings` 里没有「回退族列表」字段**（只有 `default_font`），所以「显式回退链」这件事 iced 不给你直接控制。
   来源：[docs.rs/iced/0.14.0/iced/struct.Settings.html](https://docs.rs/iced/0.14.0/iced/struct.Settings.html)

### 5) 自定义主题 / 深色 —— ✅ 支持（但内置的是配色，不是控件设计）

```rust
pub enum Theme {
    Light, Dark, Dracula, Nord, SolarizedLight, SolarizedDark,
    GruvboxLight, GruvboxDark, CatppuccinLatte, CatppuccinFrappe,
    CatppuccinMacchiato, CatppuccinMocha,
    TokyoNight, TokyoNightStorm, TokyoNightLight,
    KanagawaWave, KanagawaDragon, KanagawaLotus,
    Moonfly, Nightfly, Oxocarbon, Ferra,
    Custom(Arc<Custom>),
}
impl Theme {
    pub const ALL: &'static [Self];
    pub fn custom(name: impl Into<Cow<'static, str>>, palette: Palette) -> Self;
    pub fn custom_with_fn(name, palette, generate: impl FnOnce(Palette) -> palette::Extended) -> Self;
    pub fn palette(&self) -> Palette;
    pub fn extended_palette(&self) -> &palette::Extended;
}
```
用法（官方 crate 文档原文）：
```rust
iced::application(new, update, view).theme(theme).run()
fn theme(state: &State) -> Theme { Theme::TokyoNight }
```
还有一个环境变量后门：`ICED_THEME=<Theme::name()>`（如 `ICED_THEME="Catppuccin Mocha"`）会覆盖默认主题。
来源：[core/src/theme.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/theme.rs)、[docs.rs/iced/0.14.0/iced/enum.Theme.html](https://docs.rs/iced/0.14.0/iced/enum.Theme.html)

逐控件覆盖：每个控件有 `.style(|theme, status| ...)`，如 `button::Style::default().with_background(...)`、`text::danger`、`container::rounded_box`。
来源：[src/lib.rs crate 文档「Styling」节](https://raw.githubusercontent.com/iced-rs/iced/0.14/src/lib.rs)

### 6) 圆角 / 阴影 / 卡片 —— ✅ 支持

`container::Style` 字段（真实源码）：
```rust
pub struct Style {
    pub text_color: Option<Color>,
    pub background: Option<Background>,
    pub border: Border,
    pub shadow: Shadow,
    pub snap: bool,
}
impl Style {
    pub fn border(self, border: impl Into<Border>) -> Self;
    pub fn background(self, background: impl Into<Background>) -> Self;
    pub fn shadow(self, shadow: impl Into<Shadow>) -> Self;
}
```
现成样式函数：`container::rounded_box`（圆角 2 + `background.weak` 配色）、`container::bordered_box`（圆角 5 + 1px 边框）、`container::dark`、`primary/secondary/success/warning/danger`、`container::background(color)`。
来源：[widget/src/container.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/container.rs)

`Border` 定义在 [core/src/border.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/border.rs)（`width` / `radius: Radius` / `color`），`Shadow` 在 [core/src/shadow.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/shadow.rs)。

卡片式布局的最小配方（全部为已核实的 API 组合）：
```rust
container(row![cover_image, column![title, artist]])
    .padding(12)
    .style(|theme| container::Style {
        background: Some(theme.extended_palette().background.weak.color.into()),
        border: iced::border::rounded(12),
        shadow: iced::Shadow { color: Color::from_rgba(0.0,0.0,0.0,0.25), offset: Vector::new(0.0,2.0), blur_radius: 8.0 },
        ..Default::default()
    })
```
⚠️ **注意**：`border::rounded(12)` 的**圆角值默认只有 2（`rounded_box`）或 5（`bordered_box`）**。想要「现代 App」那种大圆角卡片，你必须自己写 `Style`——**这条正是 iced「默认丑」的核心原因**：它给你的是零件，不是设计。

## A4. 默认观感 + 可参考示例

**客观描述**：iced 的默认观感是**朴素到近乎「未设计」**——默认 `Theme::Light`/`Theme::Dark` 只定义背景色与文字色，控件（按钮/滑条/输入框）是**直角、低对比、极简**的扁平块。它**没有任何内建的「设计系统」**（对比 Slint 的 Fluent/Material/Cupertino）。圆角默认 2px、无阴影、无动效预设。切换内置主题（如 `CatppuccinMocha`）能立刻让配色变好看，但**控件形状/间距/层级依然是你自己决定**。

这也和它自己的文档口径一致——crate 文档开头就写着：

> iced is __experimental__ software. If you expect the documentation to hold your hand as you learn the ropes, you are in for a frustrating experience. ...
> Furthermore—just like Rust—iced is very unforgiving. It will not let you easily cut corners.
> ... if you feel frustrated and struggle to use the library; then I recommend you to wait patiently until [the book] is finished.

来源：[src/lib.rs](https://raw.githubusercontent.com/iced-rs/iced/0.14/src/lib.rs)

**可参考的示例/模板**：
- 官方示例目录（0.14 分支，已确认真实存在）：<https://github.com/iced-rs/iced/tree/0.14/examples>
  - 与本项目相关的：`slider`（含 `sliders.gif` 截图）、`scrollable`（含 `screenshot.png`）、`image`、`styling`（含 22 套主题的 snapshot 哈希，如 `catppuccin_mocha-tiny-skia.sha256`、`dracula-*`、`gruvbox_dark-*`、`tokyo_night-*` —— 说明官方把主题渲染当作回归测试在跑）、`pane_grid`、`modal`、`tooltip`、`pokedex`、`multi_window`、`progress_bar`
  - `styling` 快照文件名本身可作主题清单证据，列表见我抓取的 [tree API 结果](https://api.github.com/repos/iced-rs/iced/git/trees/0.14?recursive=1)
- 官方站点：<https://iced.rs>
- ⚠️ 关于「现代外观的成品模板」：**我没有核实到 iced 官方提供任何桌面应用模板**。`awesome-iced`（<https://github.com/iced-rs/awesome-iced>）是社区列表——**我本次未成功抓取其内容，故不引用其条目**。

## A5. iced 无法核实的点（明确列出）

1. **默认观感的视觉结论**：以上描述来自我读到的主题/样式源码，**我无法运行 iced 0.14 截图**，所以「丑/不丑」这句是「默认圆角 2px、无阴影、无设计系统」这些可核实事实的推论，不是视觉实证。
2. **`.ttc` 在 iced 里的端到端行为**：我证实了底层 `fontdb` 会加载 `.ttc` 的所有 face，但**没有**核实 `Font::with_name("Microsoft YaHei")` 在 Windows 上一定命中 `msyh.ttc` 里的 Regular face（face 名可能是 "Microsoft YaHei"／"微软雅黑"，且 `fontdb::Database::query` 是按 family 名匹配）。**落地时请准备 fallback：同时注册 `msyh.ttc` 与 `simhei.ttf`，或先遍历 `FaceInfo::families` 打印真实族名。**
3. **iced 是否有官方「桌面模板」仓库**：未核实到。
4. **`awesome-iced` 内容**：抓取未成功，未引用。
5. **iced 0.14 的 `examples/lazy` 是否存在**：我在 tree 输出中看到了 `examples/scrollable`、`slider`、`styling` 等，但 tree 响应被截断，**未逐一确认**。
6. **wgpu 27 的 `Backends::from_env` 是否与我在 wgpu 30 文档页读到的语义完全一致**：文档页是 wgpu 30；但 iced 0.14.0 依赖 wgpu 27。**「`WGPU_BACKEND` 环境变量名 + `dx12` 取值」在 wgpu 27 未逐字核实**（只核实了 30 的文档页与 iced 源码里确实调用了 `Backends::from_env()`）。

---

# B. Slint

## B1. 版本号 + 许可证（重点）

| 项 | 值 | 来源 |
|---|---|---|
| 最新稳定版 | **1.18.1**（2026-09-21 发布） | [crates.io API](https://crates.io/api/v1/crates/slint) |
| 许可证（crate 元数据） | `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0` | 同上 + [docs.rs/slint/1.18.1](https://docs.rs/slint/1.18.1/slint/struct.Image.html) 页头 |
| MSRV | `rust_version = "1.92"` | crates.io 1.18.1 元数据；官方 Get Started 亦写 "Slint needs Rust 1.92 or newer"（[docs.slint.dev getting-started](https://docs.slint.dev/latest/docs/slint/guide/getting-started/)） |
| 上一版 | 1.18.0 / 1.17.1；master 分支已到 **1.19.0**（未发布） | crates.io 版本列表 + [Cargo.toml](https://raw.githubusercontent.com/slint-ui/slint/master/Cargo.toml) |

### 许可证事实（逐条附原文）

**三选一，由你选择。** 官方 `LICENSE.md` 原文：

> You can use the Slint framework under ***any*** of the following licenses, at your choice:
> 1. [Royalty-free License](LICENSES/LicenseRef-Slint-Royalty-free-2.0.md) - Permits use in **proprietary** desktop, mobile, and web applications **at no cost**. Use in embedded systems is excluded.
> 2. [GNU GPLv3](LICENSES/GPL-3.0-only.txt) - Permits use in **open source software** under GPL-compatible terms, **at no cost**, ...
> 3. [Commercial license](LICENSES/LicenseRef-Slint-Software-3.0.md) - Permits use in **proprietary** applications, including desktop, mobile, web, and embedded systems. See the [pricing page](https://slint.dev/pricing) ...
>
> ... The Royalty-free License is free, as long as you disclose that you use Slint (for example with the `AboutSlint` widget or the Slint badge); without that disclosure, use the Commercial license. A Commercial license is required for embedded systems, regardless of disclosure.

来源：[LICENSE.md](https://raw.githubusercontent.com/slint-ui/slint/master/LICENSE.md)

**Royalty-free 2.0 的准确条件（原文摘录）**：

> ## 1. Grant of Rights
> SixtyFPS hereby grants You a world-wide, **royalty-free**, non-exclusive license to use, reproduce, make available, modify, display, perform, distribute the Software as part of a Desktop, Mobile, or Web Application.
>
> ## 2. License Conditions - Attribution
> You may distribute the Software as part of an Application, modified or unmodified, provided that You do either of the following:
> (a) Display the `AboutSlint` widget in an "About" screen or dialog that is accessible from the top level menu of the Application. In the absence of such a screen or dialog, display the widget in the "Splash Screen" of the Application.
> (b) Display the Slint attribution badge on a public webpage, preferably where the binaries of your Application can be downloaded from, ...
>
> ## 3. Limitations
> The License does not permit to distribute or make the Software publicly available alone and without integration into an Application. ...
> The License does not permit the use of the Software within Embedded Systems. ...
> The License does not permit the distribution of Application that exposes the APIs, in part or in total, of the Software.
> You may not remove or alter any license notices ...

来源：[LICENSES/LicenseRef-Slint-Royalty-free-2.0.md](https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md)

**对你这个项目的判定（我按原文逐条对照）**：

| 你的情况 | 判定 |
|---|---|
| 本地音乐播放器，装在用户自己的 PC 上 | ✅ 属于 **Desktop Application**，Royalty-free 覆盖 |
| 有 **royalty 吗？** | ❌ **没有**。原文 "royalty-free"，且**全文无任何营收上限/分成条款** |
| 有 **付费情形吗？** | 只有两种：① **不想署名**（必须买 Commercial）；② **嵌入式系统**（必须买 Commercial） |
| 是否必须署名 | ✅ **是**。二选一：程序内放 `AboutSlint`，**或**在公开下载页放 Slint 徽章 |
| 你自己代码的许可 | ✅ 可保持 MIT。FAQ：「Yes — whichever Slint license you use, your own source files can stay under a permissive license such as MIT or Apache-2.0.」 |
| 会不会因为「卖了 app」就要付费 | ❌ 原文没有任何收入门槛。FAQ 亦确认免费条件只有「disclose that you use Slint」 |

来源：[FAQ.md#licensing](https://raw.githubusercontent.com/slint-ui/slint/master/FAQ.md)、[Royalty-free 2.0 全文](https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md)

**⚠️ 一个必须说清楚的坑**：crate 元数据里的许可证表达式第一项是 `GPL-3.0-only`。`cargo` 本身不区分你选了哪个，**默认构建在语义上就是三选一**；你要用 Royalty-free 分支，需要**主动履行署名义务**（放 `AboutSlint`）。这不是「Cargo.toml 里改一行」能体现的，而是**产品行为上的义务**。FAQ 对「MIT 程序链接 GPLv3 Slint」说得很直白：「the work as a whole must be licensed under the GPL」——所以：**要么署名走 Royalty-free，要么整个作品 GPL，要么付费。**

### 无法核实的点
- 我**没有**抓到 `https://slint.dev/legal/`（该 URL 返回 **404**，页脚显示版权方为 SixtyFPS GmbH）。因此**「官方许可页面」的权威来源我引用的是 GitHub 仓库的 `LICENSE.md` + `LICENSES/*` + `FAQ.md` + `https://slint.dev/pricing`（未逐页核实其当前定价数字）**。
  `https://slint.dev/pricing` 我**未抓取成功**，所以**具体商业许可的价格、档位、是否含 support 一概未核实**。
- 我**未核实**「Ambassador 免费许可」是否存在（FAQ 里没有这一项，只有 Royalty-free / GPLv3 / Commercial 三者）。

## B2. 渲染后端 + Windows 上的风险

**默认设置（从 1.18.1 的 `Cargo.toml` 逐字核实）**：
```toml
default = ["std", "backend-default", "renderer-femtovg", "renderer-software",
           "accessibility", "compat-1-2", "system-tray"]
```
来源：[api/rs/slint/Cargo.toml @ v1.18.1](https://raw.githubusercontent.com/slint-ui/slint/v1.18.1/api/rs/slint/Cargo.toml)

**默认后端与渲染器**：官方 Cargo.toml 的文档注释原文：
> By default, Slint will use the [Winit](https://crates.io/crates/winit) backend with [FemtoVG](https://crates.io/crates/femtovg).
> `renderer-femtovg` is the default renderer.

**FemtoVG 用什么图形 API（官方 renderer 文档原文）**：
> ### FemtoVG Renderer
> - Highly portable.
> - **GPU acceleration with OpenGL (required).** When selected as `renderer-femtovg-wgpu`, GPU acceleration with Metal, Vulkan, and Direct3D.
> - Text and path rendering quality sometimes sub-optimal.
> - Available in the Winit backend and LinuxKMS backend.

来源：[docs.slint.dev — Backends & Renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)

### → 你的 Vulkan 问题：**默认路径完全不碰 Vulkan** ✅

对照 `SLINT_BACKEND` 取值表（官方 Winit Backend 文档原文）：

| Renderer name | Supported/Required Graphics APIs | `SLINT_BACKEND` value |
|---|---|---|
| FemtoVG | **OpenGL** | `winit-femtovg` |
| FemtoVG (WGPU) | Metal, Direct3D, **Vulkan** | `winit-femtovg-wgpu` |
| Skia | OpenGL, Metal, Direct3D, Software-rendering | `winit-skia` |
| Skia Software | Software-only | `winit-skia-software` |
| Skia OpenGL | OpenGL | `winit-skia-opengl` |
| software | Software-rendering, no GPU required | `winit-software` |

> If no renderer is explicitly set, the backend will first try to use the **Skia** renderer, **if it was enabled at compile time**. If that fails, it will fall back to the **FemtoVG** renderer, and if that also fails, it will use the **software** renderer.

来源：[docs.slint.dev — Winit Backend](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/)

**关键推论（已核实依据）**：
- 默认 feature **不含 `renderer-skia*`**，也**不含 `renderer-femtovg-wgpu`** → 所以默认编译出来的程序**永远走不到 Vulkan**。
- `renderer-skia-vulkan` 是**显式 opt-in feature**，其定义原文：`renderer-skia-vulkan = ["i-slint-backend-selector/renderer-skia-vulkan", ..., "std"]`，且官方文档明确标注 "Skia will prefer Vulkan when selecting a backend"。**你不开这个 feature，就与 Vulkan 无关。**
- `renderer-vello`（Vello/WGPU，含 Vulkan/D3D）官方标注 **Experimental: it's never selected automatically, not even when it's the only renderer compiled in**。→ 同样默认不碰。
- 兜底：`SLINT_BACKEND=winit-software` 可以强制**纯软件渲染、不需要任何 GPU**（官方表格原文 "Software-rendering, no GPU required"）。

来源：[api/rs/slint/Cargo.toml @ v1.18.1](https://raw.githubusercontent.com/slint-ui/slint/v1.18.1/api/rs/slint/Cargo.toml)、[Backends & Renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)

**Windows 支持状态（官方原文）**：

| Operating System | Architecture |
|---|---|
| Windows 10 | x86-64 |
| Windows 11 | x86-64, aarch64 |

来源：[docs.slint.dev — Windows](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/windows/general/)（同一个页面还给出 Windows 上 Rust debug 构建 `STATUS_STACK_OVERFLOW` 的官方对策：`.cargo/config.toml` 里 `rustflags = ["-C", "link-arg=/STACK:8000000"]` —— **这条对你的 rust-lld 环境同样适用，建议直接抄**）

### 已知的 FemtoVG on Windows 问题（我核实到一条，但**不适用于你**）

[slint-ui/slint#8901 「C++: Crash When Running opengl_texture Example on Windows — FemtoVG Panic」](https://github.com/slint-ui/slint/issues/8901)：
> thread '<unnamed>' panicked at .../femtovg/lib.rs:267:68: called `Option::unwrap()` on a `None` value

我通过 GitHub API 核实的元数据：`"state":"closed"`，`"state_reason":"completed"`，`"closed_at":"2025-07-11T07:43:58Z"`（**创建当天即关闭**），标签为 `a:language-c++` + `a:renderer-femtovg`。
→ 这是 **C++ + GLEW + 自定义 OpenGL 纹理互操作**示例的问题，**与「纯 Rust + 默认 feature 的普通窗口」无关**，且已关闭。但我**没有**看到修复 PR 的具体内容，所以无法断言根因。

### 无法核实的点
- FemtoVG 在 Windows 上是走 **WGL/原生 OpenGL** 还是 **ANGLE(EGL)**：官方文档只说 "OpenGL (required)"，**用哪个 GL 实现未说明**。我只在 wgpu 的文档里看到 wgpu 的 GL 后端在 Windows 上是原生 OpenGL（除非 `cfg(windows_angle)`），**这与 Slint/FemtoVG 无关，不能外推**。
- 我**没有**核实 Slint 在 Windows 上是否使用 `glutin` 创建上下文（master 的 workspace 里有 `glutin = "0.32.0"` 依赖，但那是 master/1.19，**1.18.1 是否相同未核实**）。
- **Intel 核显 OpenGL 驱动在你机器上的实际表现：完全未核实**（这需要在你机器上跑一次才知道）。

## B3. 能力 1–6 逐条

### 1) 虚拟化长列表 —— ✅ 支持，且是**官方明确承诺**的

官方 ListView 文档原文：
> In a ListView, **elements are only instantiated if they are visible**, which guarantees stable performance with a practically unlimited number of items. [ScrollView] is more suitable for free-form content.

```slint
import { ListView, VerticalBox } from "std-widgets.slint";
export component Example inherits Window {
    VerticalBox {
        ListView {
            for data in [ { text: "Blue", ... }, ... ] : Rectangle { ... }
        }
    }
}
```
来源：[docs.slint.dev — ListView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/listview/)

Rust 侧用模型驱动（官方教程原文）：
```rust
let tiles_model = std::rc::Rc::new(slint::VecModel::from(tiles));
main_window.set_memory_tiles(tiles_model.clone().into());
```
来源：[docs.slint.dev — Creating the tiles](https://docs.slint.dev/latest/docs/slint/tutorial/creating_the_tiles/)
相关类型：`slint::Model` / `VecModel` / `ModelRc` / `FilterModel` / `SortModel` / `MapModel` / `ReverseModel`（`slint` crate 根 re-export，见 [api/rs/slint/lib.rs](https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs)）

**→ 这几千首曲目，Slint 是「一个控件搞定」；iced 是「自己算可见区间」。这是两者差距最大的一条。**

### 2) 内存 RGBA / PNG 字节 → 图片 —— ✅ 支持（两种都行）

```rust
pub fn from_rgb8(buffer: SharedPixelBuffer<Rgb<u8>>) -> Image;
pub fn from_rgba8(buffer: SharedPixelBuffer<Rgba<u8>>) -> Image;
pub fn from_rgba8_premultiplied(buffer: SharedPixelBuffer<Rgba<u8>>) -> Image;
pub fn load_from_data(data: &[u8], format: Option<&str>) -> Result<Image, LoadImageError>;
pub fn load_from_path(path: &Path) -> Result<Image, LoadImageError>;
pub unsafe fn from_borrowed_gl_2d_rgba_texture(texture_id: NonZero<u32>, size: Size2D<u32, UnknownUnit>) -> Image;  // deprecated since 1.2.0
```
官方文档给的完整范例（PNG → RGBA → `Image`）：
```rust
let mut cat_image = image::open("cat.png").expect("Error loading cat image").into_rgba8();
image::imageops::colorops::brighten_in_place(&mut cat_image, 20);
let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
    cat_image.as_raw(), cat_image.width(), cat_image.height());
let image = Image::from_rgba8(buffer);
```
来源：[docs.rs/slint/1.18.1 — struct.Image](https://docs.rs/slint/latest/slint/struct.Image.html)

**⚠️ 三个必须知道的细节（都来自同一页文档）**：
1. **`Image` 不是 `Send`**（"because it uses internal cache that are local to the Slint thread"）。**封面解码必须在 UI 线程建 `Image`**：
   ```rust
   std::thread::spawn(move || {
       let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 480);
       // ... fill ...
       slint::invoke_from_event_loop(move || {
           let image = Image::from_rgba8_premultiplied(pixel_buffer);
           // my_ui_handle.upgrade().unwrap().set_image(image);
       });
   });
   ```
   → 对你的封面异步加载线程是**直接可用**的模式（你已经在用 `crossbeam-channel`）。
2. **`load_from_data` 的 feature 门槛**：docs.rs 上标注 "Available on **crate feature `image-decoders`, or WebAssembly and crate feature `std`** only"。而 1.18.1 的 `Cargo.toml` **没有 `image-decoders` 这个 feature**（只有 `image-default-formats`），且文档说明原文：
   > Supported formats are SVG, PNG and JPEG. Enable support for additional formats ... by enabling the `image-default-formats` cargo feature.
   → **默认就能解 PNG/JPEG**（`Image::load_from_data(&png_bytes, None)` 直接可用）；`image-default-formats` 只是**扩大格式**（WebP/GIF/AVIF/BMP/TIFF…）。**这一点我判断是 docs.rs 元数据与 1.18.1 Cargo.toml 的表述差异，无法 100% 排除 `load_from_data` 需要额外 feature 的可能，落地前请以一次 `cargo build` 为准（见「无法核实」）。**
3. `load_from_path` 在 Web 上永远失败（无文件系统）——桌面无影响。

### 3) 滑条 —— ✅ 支持，标准控件

```slint
import { Slider, VerticalBox } from "std-widgets.slint";
Slider { value: 42; minimum: 0; maximum: 100; step: 1; orientation: horizontal; }
```
- 属性：`enabled` / `has-focus(out)` / `value(in-out)` / `step`（默认 1，方向键步长）/ `minimum`（默认 0）/ `maximum`（默认 100）/ `orientation`（`horizontal`|`vertical`）
- 回调：`changed(float)`、`released(float)` —— **`released` 正好对应 seek 场景**（拖完才提交）
来源：[docs.slint.dev — Slider](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/slider/)

**自定义外观**：官方文档**没有**列出 `Slider` 的可替换 `handle` 子元素属性（这与 `Rectangle` 不同）。想要苹果风/极简风的进度条，官方路径是**用 `Rectangle` + `TouchArea` 自建**（官方 `fancy_demo` 示例就是「Custom widget implementations built from scratch (buttons, sliders, checkboxes, MDI windows)」，来源：[examples/README.md](https://raw.githubusercontent.com/slint-ui/slint/master/examples/README.md)）。

### 4) 中文字体（含 .ttc）—— ✅ 编译期支持 `.ttc`，运行期**无稳定 API**

官方 Font Handling 文档原文（**这是你问题的直接答案**）：
> The fonts chosen for rendering are **automatically picked up from the system** running the application. It's also possible to include custom fonts in your design. **A custom font must be a TrueType font (`.ttf`), a TrueType font collection (`.ttc`) or an OpenType font (`.otf`).** You can select a custom font with the `import` statement: `import "./my_custom_font.ttf"` in a .slint file. This instructs the Slint compiler to include the font and makes the font families globally available for use with `font-family` properties.
>
> ```
> import "./NotoSans-Regular.ttf";
> export component Example inherits Window {
>     default-font-family: "Noto Sans";
>     Text { text: "Hello World"; }
> }
> ```

来源：[docs.slint.dev — Font Handling](https://docs.slint.dev/latest/docs/slint/guide/development/fonts/)

**→ `.ttc` 官方明确支持，但方式是「编译期把字体文件编进二进制」，不是运行期从 `C:\Windows\Fonts` 读。**

对你的两个现实选项：

| 方案 | 做法 | 问题 |
|---|---|---|
| **A. 依赖系统字体族名** | 不 import，直接 `default-font-family: "Microsoft YaHei";`（或 `"微软雅黑"`） | 不需要任何字体文件；但**未核实** Slint 在 Windows 上能否正确解析中文族名 / 该 `.ttc` 里的 face 名 |
| **B. 编译期嵌入字体** | 把字体文件拷进仓库，`.slint` 里 `import "./fonts/xxx.ttf";` | ✅ 最稳（跨平台一致）。⚠️ **`msyh.ttc` 是微软的专有字体，把它拷进你的 MIT 仓库再分发属于再分发行为，需要你自行确认授权** —— 我不给法律结论。可行替代：`simhei.ttf` 同样有版权；**开源 CJK 字体（如 Noto Sans SC / Source Han Sans）授权明确，是更安全的选择**，但体积大（全量 CJK 常 >10MB，需子集化） |

**还有一个运行期 API，但它是 unstable**：
```rust
#[cfg(feature = "unstable-fontique-011")]
pub mod fontique_011 {
    pub fn shared_collection() -> fontique::Collection { ... }
}
```
官方文档原文：**"Note: The recommended way of including custom fonts is at compile time of Slint files."**，且 feature 名带第三方 crate 版本号，官方声明 **"*NOT* subject to the usual Slint API stability guarantees: a future minor release of Slint may change or remove them"**。
来源：[api/rs/slint/lib.rs](https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs)（`unstable-fontique-011` 模块 + Cargo.toml 的 unstable features 说明）
⚠️ 这是 **master/1.19** 的文档；**1.18.1 是否已有 `unstable-fontique-011` 我未逐一核实**（1.18.1 的 docs.rs features 页列为 `unstable-fontique-011`，见 [crates.io 1.18.1 features](https://crates.io/api/v1/crates/slint)）。

**→ 结论：Slint 的中文字体方案比 iced 麻烦，且有一个「编译期 vs 运行期」的路线选择要做。你现有的 `platform::cjk_font_candidates()` + `load_first_available_font()` 那套运行期探测，在 Slint 里用不上（除非走 unstable API）。**

### 5) 自定义主题 / 深色 —— ✅ 支持，而且**控件真的会跟着变**

```slint
// Palette 是内建 global，属性（官方文档）
background, foreground                     // 默认背景/前景
alternate-background, alternate-foreground  // 输入框、侧边栏这类面板
control-background, control-foreground      // push button / combo box 等控件
accent-background, accent-foreground        // primary button 等高亮
selection-background, selection-foreground  // 文本选中
border                                      // 分隔线、控件边框
color-scheme (in-out)                       // ColorScheme 枚举，可读可写
```
> Read this property to determine the color scheme used by the palette. **Set this property to force a dark or light color scheme.** All styles except for the Qt style support setting a dark or light color scheme.

`ColorScheme` 枚举：`unknown` / `dark` / `light`（`unknown` = 跟随系统）。
来源：[docs.slint.dev — Palette](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/globals/palette/)

**6 种控件样式 + 明暗变体**：

| Style Name | Light | Dark | 说明 |
|---|---|---|---|
| `fluent` | `fluent-light` | `fluent-dark` | 基于 **Fluent Design System** |
| `material` | `material-light` | `material-dark` | 遵循 **Material Design** |
| `cupertino` | `cupertino-light` | `cupertino-dark` | 仿 macOS |
| `cosmic` | `cosmic-light` | `cosmic-dark` | 仿 Cosmic Desktop |
| `qt` | — | — | 需要装 Qt |
| `native` | — | — | Windows 上 = `fluent` |

> By default, the styles automatically adapt to the system's dark or light color setting. Select a `-light` or `-dark` variant to override ...
> **The widget style is determined at your project's compile time.** ... You can select the style before starting your compilation by setting the **`SLINT_STYLE`** environment variable to the name of your chosen style. When using the `slint_build` API, call `slint_build::compile_with_config()`.
> **If no style is selected, `fluent` is the default on all platforms.**

来源：[docs.slint.dev — Widget Styles](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/)

→ **「深色音乐播放器」在 Slint 里是**：`SLINT_STYLE=fluent-dark`（编译期）或 `.slint` 里设 `Palette.color-scheme: dark;`，**然后所有标准控件自动变深色**。对比 iced：换主题只换配色，控件形状不变。

### 6) 圆角 / 阴影 / 卡片 —— ✅ 支持，属性齐全

`Rectangle` 的全部相关属性（官方文档目录逐字核实）：
- 基础：`background`、`border-color`、`border-width`、`clip`
- 圆角：`border-radius`、`border-top-left-radius`、`border-top-right-radius`、`border-bottom-left-radius`、`border-bottom-right-radius`
- **投影**：`drop-shadow-blur`、`drop-shadow-color`、`drop-shadow-offset-x`、`drop-shadow-offset-y`、`drop-shadow-spread`
- **内阴影**：`inner-shadow-blur`、`inner-shadow-color`、`inner-shadow-offset-x`、`inner-shadow-offset-y`、`inner-shadow-spread`

官方示例原文：
```slint
Rectangle {
    width: 180px; height: 180px;
    border-width: 4px; border-color: black;
    border-radius: 30px;          // 圆角
}
Rectangle { border-radius: self.width/2; }   // 半径=宽/2 即圆形（做圆形封面！）
```
来源：[docs.slint.dev — Rectangle](https://docs.slint.dev/latest/docs/slint/reference/elements/rectangle/)

**⚠️ 但软件渲染器有明确限制**（官方原文，与「圆角卡片」直接冲突）：
> ### Software Renderer
> Some features haven't been implemented yet:
> - No support for rotations or scaling.
> - **No support for `drop-shadow-*` properties.**
> - **No support for `border-radius` in combination with `clip: true`.**
> - No text stroking/outlining.
> - **Text rendering currently limited to western scripts.**

来源：[Backends & Renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)

**这三条对你有直接影响**：
1. 如果你为了躲开显卡问题而用 `SLINT_BACKEND=winit-software`，**卡片阴影全部消失**，且「圆角 + `clip:true`（裁剪封面）」失效。
2. **"Text rendering currently limited to western scripts"** → 软件渲染器下**中文可能渲染不出来**。所以**软件渲染不能作为你的中文方案兜底**，只能靠 FemtoVG/OpenGL 正常工作。

### B3 附：Rust 嵌入方式（官方 Quickstart 原文，可直接抄）

**方式一：`.slint!` 宏（内联）**
```rust
slint::slint!{
    export component HelloWorld inherits Window {
        Text { text: "hello world"; color: green; }
    }
}
fn main() { HelloWorld::new().unwrap().run().unwrap(); }
```

**方式二：外部 `.slint` 文件 + `build.rs`（推荐，也是「Rust 写逻辑 / .slint 写 UI」的标准形态）**

`Cargo.toml`：
```toml
[package]
build = "build.rs"
edition = "2021"

[dependencies]
slint = "1.18"

[build-dependencies]
slint-build = "1.18"
```

`build.rs`：
```rust
fn main() {
    slint_build::compile("ui/hello.slint").unwrap();
}
```

`main.rs`：
```rust
slint::include_modules!();   // 宏定义：include!(env!("SLINT_INCLUDE_GENERATED"));
fn main() { HelloWorld::new().unwrap().run().unwrap(); }
```

生成的 Rust API 约定（官方文档原文）：
- 每个顶层属性 → `get_<name>()` / `set_<name>()`
- 每个顶层回调 → `on_<name>(impl Fn(Args) + 'static)` / `invoke_<name>(args)`
- `ComponentHandle` 提供 `new()` / `show()` / `hide()` / `run()` / `as_weak()` / `clone_strong()` / `global::<T>()`
- **"All dashes (`-`) are replaced by underscores (`_`) in names"**
- ⚠️ **"The generated component struct acts as a handle holding a strong reference (similar to an `Rc`). The `Clone` trait is not implemented."** 且 **"A strong reference should not be captured by the closures given to a callback, as this would produce a reference loop and leak the component. Instead, the callback function should capture a weak component."**

来源：[api/rs/slint/lib.rs](https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs)、[docs.rs/slint/latest/slint/macro.include_modules.html](https://docs.rs/slint/latest/slint/macro.include_modules.html)

**官方模板仓库**：<https://github.com/slint-ui/slint-rust-template>（官方 Get Started 页推荐的骨架）

**⚠️ 构建环境相关（你关心「没装 Visual Studio」）**：
- 官方 Get Started 页的「Install the Prerequisites / Rust」只需 Rust 工具链，原文：**"The Slint crate builds as part of your project, so no separate installation of Slint is needed."**
- `slint-build` 编译 `.slint` 是**纯 Rust 编译器**（`i-slint-compiler`），默认 feature 集**不含 Skia / Qt**，所以**不需要 CMake、不需要 C++ 编译器、不需要下载 Skia**。
- ⚠️ **但我没有找到一个官方页面明确写「Windows 上默认 feature 不需要 C++ 工具链」**——这是我从「默认 feature 不含 skia/qt」推出来的（推理依据：1.18.1 `Cargo.toml` 的 `default` 列表里没有 `renderer-skia`、没有 `backend-qt`）。**落地前请以一次真实 `cargo build` 验证。**
- 官方 Windows 页给出一条你**应该直接采纳**的配置（Rust debug 构建栈溢出）：
  ```toml
  # .cargo/config.toml
  [target.x86_64-pc-windows-msvc]
  rustflags = ["-C", "link-arg=/STACK:8000000"]
  ```
  来源：[docs.slint.dev — Windows](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/windows/general/)

### B3 附：事件循环/线程约束（对你的播放器架构有影响）

官方原文：
> For platform-specific reasons, **the event loop must run in the main thread**, in most backends, and **all the components must be created in the same thread as the thread the event loop is running** or is going to run. You should perform the minimum amount of work in the main thread and delegate the actual logic to another thread ... Use the `invoke_from_event_loop` function to communicate from your worker thread to the UI thread.

以及 **`spawn_local` 与 Tokio 的兼容坑**（原文很长，核心）：
> Tokio futures aren't guaranteed to hand off their work to separate threads and may therefore not complete, because the Slint runtime can't drive the Tokio runtime. ... **The use of `#[tokio::main]` is not recommended.**

来源：[api/rs/slint/lib.rs](https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs)
→ 你现在用的是 `crossbeam-channel` + `rayon`，**没有 Tokio**，所以这块**对你不是问题**。

## B4. 默认观感 + Gallery

**客观描述**：Slint 的默认观感是**经过设计的、成体系的**——默认样式就是 **Fluent**（Microsoft Fluent Design System），Windows 上 `native` 别名也指向 `fluent`。它是**一套完整的控件设计**（按钮有 hover/pressed 态、下拉框、开关、Tab、滚动条都有统一外观），不是「一片扁平方块」。而且**深色是官方一等公民**（`Palette.color-scheme: dark` + `fluent-dark`，且默认自动跟随系统明暗）。

**官方 Gallery / 示例**（全部从官方 [examples/README.md](https://raw.githubusercontent.com/slint-ui/slint/master/examples/README.md) 逐条核实，含官方 wasm 在线 demo 链接）：

| 示例 | 说明 | 在线 demo |
|---|---|---|
| **Widget Gallery** (`gallery`) | 展示各种控件的窗口 | <https://slint.dev/snapshots/master/demos/gallery/> |
| **Todo** (`todo`, `todo-mvc`) | 简易待办（MVC 版） | <https://slint.dev/snapshots/master/demos/todo/> |
| **Image Filter** (`imagefilter`) | **Rust-only：用 image crate 处理图片再喂给 Slint** ← 与你的封面场景最像 | <https://slint.dev/snapshots/master/demos/imagefilter/> |
| **iot-dashboard** | 仪表盘（QSkinny 移植） | — |
| **Plotter** (`plotter`) | Rust + plotters 集成 | <https://slint.dev/snapshots/master/demos/plotter/> |
| **Custom Title Bar** (`custom-titlebar`) | **无边框窗口 + 自定义标题栏（移动/缩放/最小化/最大化/关闭）** ← 想做「现代感音乐播放器」必看 | — |
| **Carousel** (`carousel`) | 触摸/鼠标/键盘可控的轮播 | <https://slint.dev/snapshots/master/demos/carousel/> |
| **dnd-kanban** | 拖拽看板（卡片跨列拖动） | <https://slint.dev/snapshots/master/demos/dnd-kanban/> |
| **fancy_demo** | **从零手写自定义控件（按钮、滑条、复选框、MDI 窗口）** ← 想自定义进度条看这个 | — |
| **fancy-switches**、`dial`、`speedometer`、`orbit-animation`、`sprite-sheet` | 动效/仪表控件 | — |
| `maps`、`ffmpeg`、`wgpu_texture`、`virtual_keyboard`、`7guis` | 进阶集成 | — |

- **官方 Printer Demo**（`slint-viewer` 文档页展示的截图示例）：<https://github.com/slint-ui/slint/blob/master/demos/printerdemo/ui/printerdemo.slint>
- **在线试玩**：<https://slintpad.com>（官方文档中每个示例都有 "Open in SlintPad" 链接）
- **Showcase**（成品应用展示页）：<https://slint.dev/showcase.html>、<https://slint.dev/demos.html>

⚠️ **我未找到官方的「music player」示例**（`examples/README.md` 里没有）。最接近的是 `ffmpeg`（渲染视频帧）和 `imagefilter`（Rust 处理图片 → Slint 显示）。

## B5. Slint 无法核实的点（明确列出）

1. **`msyh.ttc` 的族名匹配**：官方文档说系统字体会自动加载、`.ttc` 可 import，但我**未核实** `default-font-family: "Microsoft YaHei"` 在你的 Windows 10 上是否会命中 `msyh.ttc` 里的 Regular face（face 级名称可能不同）。**落地必做：先用 `slint-viewer` 打开一个纯 Text 的 `.slint` 试一下。**
2. **`Image::load_from_data` 的 feature 门槛**：docs.rs 标注需要 `image-decoders`，但 1.18.1 的 `Cargo.toml` 里没这个 feature；文档正文又说默认支持 SVG/PNG/JPEG。**这个矛盾我无法从源码层面判定**，请以一次编译为准。（`Image::from_rgba8` 路径**无此疑问**，可以放心用——而这正好是你 `src/covers.rs` 已有的 RGBA 数据。）
3. **Windows 上默认 feature 是否真的零 C++/CMake 依赖**：我只有「默认 feature 不含 skia/qt」这个间接证据，**没有官方明确声明**。
4. **`unstable-fontique-011` 在 1.18.1 中是否已存在**：在 1.18.1 的 crates.io feature 列表里看到了，但代码文档我读的是 master（1.19）。**未核实 1.18.1 的该 API 签名**。
5. **商业许可的具体价格/档位/是否含 support**：`https://slint.dev/pricing` **未抓取成功**。
6. **`https://slint.dev/legal/` 返回 404**，所以「官方许可页面」我引用的是仓库内 `LICENSE.md`/`LICENSES/*`/`FAQ.md`。我**无法确认 slint.dev 网站上是否另有一处更新的许可条款页**。
7. **FemtoVG 在 Windows 用的是 WGL 还是 ANGLE**：未核实（见 B2）。
8. **`Slider` 是否支持通过子元素替换 `handle`**：官方 Slider 文档**未列出**此类属性，我据此判断「不支持/需自建」，但**未核实源码**。
9. **默认观感的视觉实证**：同 iced，我读的是官方文档与示例截图链接，**没有在自己机器上截过图**。

---

# 最终结论（直说）

## 哪个更强：看你要优化什么

**如果你的目标是「花最少时间让界面变好看」→ Slint 明显更强。**

理由不是主观的，是可核实的：
1. 它**自带一套设计系统**。默认 `fluent`，还能一行环境变量切 `material` / `cupertino` / `cosmic`，而且**控件真的会跟着变**（对比 iced：22 套主题只是配色，控件形状全靠你写）。
2. **深色是一等公民**（`Palette.color-scheme: dark`），对音乐播放器这种几乎必定深色的场景，省掉一大堆调色工作。
3. **`ListView` 官方承诺只实例化可见项**——你那几千首曲库是**一个控件**的活。iced 那边你要自己算可见区间 + `lazy`。
4. **`Rectangle` 自带 `drop-shadow-*` / `inner-shadow-*` / `border-radius`**，做「封面卡片 + 悬浮阴影 + 圆形封面（`border-radius: width/2`）」是声明式的几行。
5. **默认不碰 Vulkan**。你的 Intel `igvk64.dll` 崩溃问题，在 Slint 默认配置下**根本不会发生**（默认只有 FemtoVG/OpenGL + 软件回退；Skia-Vulkan 是你要主动开的 feature）。
6. 有 `custom-titlebar`（无边框窗口）和 `imagefilter`（Rust 处理图片 → Slint 显示）两个**与你需求几乎重合**的官方示例。

**代价（必须接受这 5 条）：**
1. **引入 `.slint` DSL**。你不再只写 Rust——多了一门声明式语言、一个 `build.rs` 代码生成步骤、`.slint` 里的编译期类型错误。
2. **署名义务**。想白用就得放 `AboutSlint` 或官网挂徽章。**不想署名 = 付费**。没有收入门槛，只有「署名 / 付费 / GPL」三选一。
3. **中文字体要重新设计**。你最优雅的 `platform::cjk_font_candidates()` + 运行期读 `msyh.ttc` 这套**用不上**了（运行期 API 是 unstable）。要么依赖系统族名（未核实风险），要么**编译期嵌入**（体积 + 字体授权要自己解决）。**这是你从 egui 迁到 Slint 最实际的一次返工。**
4. **软件渲染器救不了中文**。官方明说软件渲染 "Text rendering currently limited to western scripts"，且不支持 `drop-shadow-*`、不支持「圆角 + clip」。**所以你不能把 `winit-software` 当兜底方案**——一旦 FemtoVG 在你机器上出问题，你没有安全网（iced 有 tiny-skia，但也有同样的 CJK 风险）。
5. **Rust 1.92+**（你 `rust-version = "1.95"`，没问题）。

## 什么时候应该选 iced

**如果「零许可证摩擦 / 纯 Rust / 完全掌控渲染」比「快速好看」更重要 → iced。**

它的优势同样可核实：
1. **MIT，无条件**，不用署名、不用掂量商业模式。
2. **纯 Rust 栈**，和你现有的 `platform` / `covers` / `db` 层衔接最自然——尤其 `Application::font(bytes)` 吃 `Cow<'static, [u8]>`，**你 `platform::load_first_available_font()` 返回的 `LoadedFont.bytes` 可以原样喂进去**。
3. **`WGPU_BACKEND=dx12` 是有官方文档背书的 Vulkan 逃生门**（wgpu 文档明列 `dx12`/`d3d12`），而且默认就带 tiny-skia 软件回退。**你的 Intel 驱动问题有确定解。**
4. `Slider::on_release` 天然解决 seek 抖动；`container::Style` + `Shadow` + `Border::radius` 能做出任何卡片。
5. 22 套内置配色里 `CatppuccinMocha` / `TokyoNight` / `Dracula` 都是现代深色，**换一行就有不错的配色基线**。

**代价：**
1. **默认真的丑**。圆角默认 2px、无阴影、无设计系统。你现在的「用户骂太丑」，换成 iced **默认状态大概率还是会被骂**——**除非你愿意认真写一套 `container::Style` / 各控件 `.style()` 的主题层**。这是实打实的工作量，比 Slint 的 `SLINT_STYLE=fluent-dark` 贵得多。
2. **虚拟列表要自己实现**（可见区间算法 + `lazy`）。`widget::table` 会构建全部行，**不能当曲库列表用**。
3. 它自己的官方文档就写着 *"very unforgiving"*、*"it will not let you easily cut corners"*、学不动就 *"wait patiently until the book is finished"*。

## 一句话

- **要「好看」→ Slint**，代价是学 `.slint` + 署名 + 重做中文字体。
- **要「省心合法」→ iced**，代价是**你自己动手写设计系统**，否则丑的问题原样复发。
- **不要选 eframe/egui 继续硬扛**（用户已经给过判决）。
- **两者都不碰 Vulkan 的方式我都已核实**：iced 用 `WGPU_BACKEND=dx12`；Slint 默认就不碰（且**别**开 `renderer-skia-vulkan` / `renderer-femtovg-wgpu` / `renderer-vello`）。

## 落地前必须先做的两个 5 分钟验证（因为它们是我明确无法核实的点）

1. **Slint + 中文**：`cargo binstall slint-viewer`（或 `cargo install slint-viewer`），写一个只有 `Text { text: "曲库 播放 专辑"; }` 的 `.slint`，跑起来看中文是否正常 + `default-font-family: "Microsoft YaHei"` 是否命中。
2. **iced + DX12**：建一个 hello-world，**先不设** `WGPU_BACKEND` 看是否复现 Intel Vulkan 崩溃，再 `set WGPU_BACKEND=dx12` 看是否稳定。这一步能直接决定 iced 是否可行。

---

## 附：核实过的 URL 清单

**iced**
- <https://crates.io/api/v1/crates/iced>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/Cargo.toml>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/lib.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/renderer/src/fallback.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/lib.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/settings.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/wgpu/src/window/compositor.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/settings.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/theme.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/image.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/core/src/font.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/graphics/src/text.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lib.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lazy.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/lazy/helpers.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/table.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/slider.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/container.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/widget/src/image.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/src/lib.rs>
- <https://raw.githubusercontent.com/iced-rs/iced/0.14/src/application.rs>
- <https://api.github.com/repos/iced-rs/iced/git/trees/0.14?recursive=1>
- <https://docs.rs/iced/0.14.0/iced/struct.Settings.html>
- <https://docs.rs/iced/0.14.0/iced/struct.Font.html>
- <https://docs.rs/wgpu/latest/wgpu/struct.Backends.html>
- <https://wgpu.rs/doc/wgpu_types/backend/enum.Backend.html>
- <https://raw.githubusercontent.com/RazrFalcon/fontdb/master/src/lib.rs>（`.ttc` 多 face 装载逻辑）

**Slint**
- <https://crates.io/api/v1/crates/slint>
- <https://raw.githubusercontent.com/slint-ui/slint/master/LICENSE.md>
- <https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md>
- <https://raw.githubusercontent.com/slint-ui/slint/master/FAQ.md>
- <https://raw.githubusercontent.com/slint-ui/slint/v1.18.1/api/rs/slint/Cargo.toml>（默认 feature / 各 renderer feature 定义）
- <https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs>
- <https://raw.githubusercontent.com/slint-ui/slint/master/Cargo.toml>
- <https://raw.githubusercontent.com/slint-ui/slint/master/examples/README.md>
- <https://docs.rs/slint/latest/slint/struct.Image.html>
- <https://docs.rs/slint/latest/slint/macro.include_modules.html>
- <https://docs.slint.dev/latest/docs/slint/guide/getting-started/>
- <https://docs.slint.dev/latest/docs/slint/guide/development/fonts/>（**`.ttc` 支持出处**）
- <https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/>
- <https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/>
- <https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/windows/general/>
- <https://docs.slint.dev/latest/docs/slint/guide/tooling/manual-setup/>
- <https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/listview/>
- <https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/slider/>
- <https://docs.slint.dev/latest/docs/slint/reference/std-widgets/globals/palette/>
- <https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/>
- <https://docs.slint.dev/latest/docs/slint/reference/elements/rectangle/>
- <https://docs.slint.dev/latest/docs/slint/tutorial/creating_the_tiles/>
- <https://api.github.com/repos/slint-ui/slint/issues/8901>
