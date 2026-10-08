# eframe / egui 0.36.2 — source-verified API reference

Verified against the **actual published 0.36.2 crate sources** (downloaded from crates.io and read
locally) plus docs.rs and the GitHub `0.36.2` tag. Rust toolchain on this machine is
`rustc 1.99.0 (b940084d7 2026-09-28)`, host `x86_64-pc-windows-msvc` — same as yours.

> **⚠️ Compile status — read this first.**
> I could **not** run `cargo check` / `cargo build` on this machine: there is no MSVC linker
> (`link.exe`) and no Windows SDK, so build scripts and proc-macros
> (`bytemuck_derive`, `serde_derive`, `thiserror-impl`, `zerocopy-derive`) cannot be linked.
> Proven directly:
> ```
> error: linker `link.exe` not found
> note: the msvc targets depend on the msvc linker but `link.exe` was not found
> error: could not compile `bstest` (build script) due to 1 previous error
> ```
> `rust-lld` is present but fails for the same reason (`rust-lld: error: could not open 'kernel32.lib'`).
> So **every signature below is read out of the real 0.36.2 source, not compiler-confirmed.**
> The one item I flag as *inferred* is called out in §4.

Sources used (all fetched, HTTP 200):

- https://docs.rs/eframe/0.36.2/eframe/
- https://docs.rs/eframe/0.36.2/eframe/struct.NativeOptions.html
- https://docs.rs/eframe/0.36.2/eframe/trait.App.html
- https://docs.rs/eframe/0.36.2/eframe/fn.run_native.html
- https://docs.rs/eframe/0.36.2/eframe/type.AppCreator.html
- https://raw.githubusercontent.com/emilk/egui/0.36.2/crates/eframe/src/lib.rs
- https://raw.githubusercontent.com/emilk/egui/0.36.2/crates/eframe/src/epi.rs
- https://raw.githubusercontent.com/emilk/egui/0.36.2/crates/eframe/src/icon_data.rs
- https://raw.githubusercontent.com/emilk/egui/0.36.2/examples/hello_world_simple/src/main.rs
- https://raw.githubusercontent.com/emilk/egui/0.36.2/CHANGELOG.md
- https://raw.githubusercontent.com/emilk/egui/0.36.2/crates/eframe/CHANGELOG.md
- https://raw.githubusercontent.com/emilk/egui/0.36.2/crates/epaint/CHANGELOG.md
- https://github.com/emilk/eframe_template/blob/main/src/main.rs
- Published crate tarballs: `https://static.crates.io/crates/{eframe,egui,egui_extras,epaint,ecolor,emath}/<name>-0.36.2.crate`
  (also `epaint-0.29.1.crate` and `epaint-0.27.1.crate` for the 0.27–0.29 comparison)

---

## 0. The headline: there is no `update` method any more

**`eframe::App`'s required method is `ui`, not `update`.** `App::update` was deprecated in 0.34 and
then **deleted** in 0.35 ("Remove everything that was marked `#[deprecated]`" — PR #8105).

The *entire* trait, verbatim from `crates/eframe/src/epi.rs` @ `0.36.2`:

```rust
pub trait App {
    // Required method
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame);

    // Provided methods
    fn logic(&mut self, ctx: &egui::Context, frame: &mut Frame) { _ = (ctx, frame); }
    fn save(&mut self, _storage: &mut dyn Storage) {}
    fn on_exit(&mut self, _gl: Option<&glow::Context>) {}   // #[cfg(feature = "glow")]
    fn on_exit(&mut self) {}                                 // #[cfg(not(feature = "glow"))]
    fn auto_save_interval(&self) -> core::time::Duration { … 30s … }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { … }
    fn persist_egui_memory(&self) -> bool { true }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw_input: &mut egui::RawInput) {}
    // + fn as_any_mut(&mut self) -> Option<&mut dyn Any>   // wasm32 only
}
```

The docs.rs page agrees — "Required Methods: `ui`", "Provided Methods: `auto_save_interval`,
`clear_color`, `logic`, `on_exit`, `persist_egui_memory`, `raw_input_hook`, `save`".

`App::ui` receives a whole-app `Ui`, and **`Ui` derefs to `Context`** (`impl Deref for Ui { type Target = Context; }`,
`egui-0.36.2/src/ui.rs:92`), so `ui.input(…)`, `ui.request_repaint()`, `ctx.load_texture(…)` via `ui`
all work. The `ui` you are given has **no margin and no background** — wrap in `CentralPanel`.

---

## 1. Minimal app skeleton

`run_native` (source: `crates/eframe/src/lib.rs:288`):

```rust
pub fn run_native(
    app_name: &str,
    native_options: NativeOptions,
    app_creator: AppCreator<'_>,
) -> Result
```

`AppCreator` (`crates/eframe/src/epi.rs:49-50`):

```rust
pub type AppCreator<'app> =
    Box<dyn 'app + FnOnce(&CreationContext<'_>) -> Result<Box<dyn 'app + App>, DynError>>;
// where type DynError = Box<dyn core::error::Error + Send + Sync>;
```

`eframe::Result` (`crates/eframe/src/lib.rs:617`): `pub type Result<T = (), E = Error> = core::result::Result<T, E>;`
so `-> eframe::Result` means `Result<(), eframe::Error>`.

Note the closure must return `Ok(Box::new(app))` — the `Result` wrapper has been there since 0.28
(eframe CHANGELOG 0.28.0, "Wrap app creator in a `Result`"), so it is not new in 0.36.

### Complete compiling hello-world

This is the official `examples/hello_world_simple/src/main.rs` @ `0.36.2`, verbatim:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![expect(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    // Our application state:
    let mut name = "Arthur".to_owned();
    let mut age = 42;

    eframe::run_ui_native("My egui App", options, move |ui, _frame| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("My egui Application");
            ui.horizontal(|ui| {
                let name_label = ui.label("Your name: ");
                ui.text_edit_singleline(&mut name)
                    .labelled_by(name_label.id);
            });
            ui.add(egui::Slider::new(&mut age, 0..=120).text("age"));
            if ui.button("Increment").clicked() {
                age += 1;
            }
            ui.label(format!("Hello '{name}', age {age}"));
        });
    })
}
```

And the struct-based form, from the `run_native` doc comment (`crates/eframe/src/lib.rs:256-281`):

```rust
use eframe::egui;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("MyApp", native_options, Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))))
}

#[derive(Default)]
struct MyEguiApp {}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_global_style.
        Self::default()
    }
}

impl eframe::App for MyEguiApp {
   fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
       egui::CentralPanel::default().show(ui, |ui| {
           ui.heading("Hello World!");
       });
   }
}
```

If you only depend on `eframe` (not `egui` directly), you must `use eframe::egui;` first —
`eframe` re-exports `pub use egui;`. `eframe_template`'s `Cargo.toml` declares `egui = "0.36.0"`
as a **direct** dependency, which is why its `main.rs` can say `egui::ViewportBuilder` with no import.

There is also a one-liner API, `run_ui_native` (`crates/eframe/src/lib.rs:478`):

```rust
pub fn run_ui_native(
    app_name: &str,
    native_options: NativeOptions,
    ui_fun: impl FnMut(&mut egui::Ui, &mut Frame) + 'static,
) -> Result
```

---

## 2. `NativeOptions` fields for a music player

Verbatim struct definition (docs.rs `struct.NativeOptions.html` and `crates/eframe/src/epi.rs`,
`#[cfg(not(target_arch = "wasm32"))]`), 14 fields:

```rust
pub struct NativeOptions {
    pub viewport: ViewportBuilder,
    pub multisampling: u16,
    pub depth_buffer: u8,
    pub stencil_buffer: u8,
    pub renderer: Renderer,                              // cfg(glow or wgpu_no_default_features)
    pub run_and_return: bool,
    pub event_loop_builder: Option<EventLoopBuilderHook>,
    pub window_builder: Option<WindowBuilderHook>,
    pub centered: bool,
    pub glow_options: GlowConfiguration,                 // cfg(feature = "glow")
    pub wgpu_options: WgpuConfiguration,                 // cfg(feature = "wgpu_no_default_features")
    pub persist_window: bool,
    pub persistence_path: Option<PathBuf>,
    pub dithering: bool,
}
```

**There is no `initial_window_size` and no `title` field** — they moved into `viewport` back in 0.24.
The doc for the `viewport` field says: *"This is where you set things like window title and size."*

There is **no `icon` field on `NativeOptions`** either — the icon lives on `ViewportBuilder`.

Defaults worth knowing: `centered: false`, `persist_window: true`, `run_and_return: true`,
`dithering: true`, `multisampling/depth_buffer/stencil_buffer: 0`.

### Exact struct-literal syntax

```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_title("Echo")
        .with_app_id("echo")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 500.0])
        .with_icon(icon),          // see §3
    centered: true,
    ..Default::default()
};
```

Verified `ViewportBuilder` builder methods (`egui-0.36.2/src/viewport.rs`):

| method | exact signature | line |
|---|---|---|
| `with_title` | `pub fn with_title(mut self, title: impl Into<String>) -> Self` | 355 |
| `with_inner_size` | `pub fn with_inner_size(mut self, size: impl Into<Vec2>) -> Self` | 531 |
| `with_min_inner_size` | `pub fn with_min_inner_size(mut self, size: impl Into<Vec2>) -> Self` | 544 |
| `with_icon` | `pub fn with_icon(mut self, icon: impl Into<Arc<IconData>>) -> Self` | 434 |
| `with_app_id` | `pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self` | 645 |
| `with_resizable` | `pub fn with_resizable(mut self, resizable: bool) -> Self` | 400 |
| `with_decorations` | `pub fn with_decorations(mut self, decorations: bool) -> Self` | 366 |
| `with_maximized` | `pub fn with_maximized(mut self, maximized: bool) -> Self` | 389 |
| `with_transparent` | `pub fn with_transparent(mut self, transparent: bool) -> Self` | 424 |
| `with_visible` | `pub fn with_visible(mut self, visible: bool) -> Self` | 460 |
| `with_active` | `pub fn with_active(mut self, active: bool) -> Self` | 449 |
| `with_taskbar` | `pub fn with_taskbar(mut self, show: bool) -> Self` | 519 |
| `with_monitor` | (added 0.35, per egui CHANGELOG "#8140") | — |

`with_inner_size` takes `impl Into<Vec2>`, so `[1280.0, 800.0]` (a `[f32; 2]`) works, as does
`egui::vec2(1280.0, 800.0)`.

`with_app_id` also picks the persistence folder. Per the `NativeOptions` docs: *"If you don't set an
app id, the title argument to `crate::run_native` will be used as app id instead."* (eframe 0.34.3
CHANGELOG: "Default `app_id` to `app_name` on native", #8172.)

### Setting the title at runtime

`Frame` no longer carries commands; use viewport commands:

```rust
ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title("Echo — Now Playing".to_owned()));
ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280.0, 800.0)));
ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
```

---

## 3. Window / app icon

`egui::IconData` is a **plain public-field struct** (`egui-0.36.2/src/viewport.rs:181-192`):

```rust
#[derive(Clone, Default, PartialEq, Eq)]
pub struct IconData {
    /// RGBA pixels, with separate/unmultiplied alpha.
    pub rgba: Vec<u8>,
    /// Image width. This should be a multiple of 4.
    pub width: u32,
    /// Image height. This should be a multiple of 4.
    pub height: u32,
}
```

**Important: `IconData` has NO `from_rgba_unmultiplied` constructor in 0.36.2.** The only inherent
method is `pub fn is_empty(&self) -> bool`. You build it as a struct literal:

```rust
fn icon_from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> egui::IconData {
    egui::IconData { rgba, width, height }
}
// Note the field order in the struct is rgba, width, height — order in the literal is irrelevant.
```

From a PNG, use the free function in `eframe::icon_data` (`crates/eframe/src/icon_data.rs:24`):

```rust
pub fn from_png_bytes(png_bytes: &[u8]) -> Result<IconData, image::ImageError>
```

`eframe_template` @ `main` (targets `egui`/`eframe` **0.36.0**) uses exactly this:

```rust
let native_options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([400.0, 300.0])
        .with_min_inner_size([300.0, 220.0])
        .with_icon(
            // NOTE: Adding an icon is optional
            eframe::icon_data::from_png_bytes(
                &include_bytes!("../assets/favicon-512x512.png")[..],
            )
            .expect("Failed to load icon"),
        ),
    ..Default::default()
};
```

`with_icon` takes `impl Into<Arc<IconData>>`; `Arc<IconData>: From<IconData>` via the std blanket
impl, so passing a bare `IconData` value works.

Two more verified facts:

- `eframe::icon_data` also has an extension trait `IconDataExt` with `to_image(&self) -> Result<image::RgbaImage, String>`
  and `to_png_bytes(&self) -> Result<Vec<u8>, String>` (implemented for `egui::IconData`).
- There is also `impl From<IconData> for epaint::ColorImage` and `impl From<&IconData> for epaint::ColorImage`,
  both using `from_rgba_premultiplied` (`egui-0.36.2/src/viewport.rs:210-224`) — irrelevant for the
  window icon, but note egui *premultiplies* here even though the field is documented as
  "separate/unmultiplied alpha".
- Default icon behaviour: *"If you don't set an icon, a default egui icon will be used. To avoid this,
  set the icon to `egui::IconData::default()`."*
- Runtime icon change: `egui::ViewportCommand::Icon(Option<Arc<IconData>>)` (`viewport.rs:1156`).

---

## 4. Images

### `ColorImage` — and a real trap

`ColorImage` in 0.36.2 (`epaint-0.36.2/src/image.rs:45-57`):

```rust
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ColorImage {
    /// width, height in texels.
    pub size: [usize; 2],
    /// Size of the original SVG image (if any), or just the texel size of the image.
    pub source_size: Vec2,          // <-- NEW third field
    /// The pixels, row by row, from top to bottom.
    pub pixels: Vec<Color32>,
}
```

`ColorImage::from_rgba_unmultiplied` **still exists, unchanged**, and this signature is identical in
0.27.1, 0.29.1 and 0.36.2:

```rust
pub fn from_rgba_unmultiplied(size: [usize; 2], rgba: &[u8]) -> Self
```

It **panics** (not `Result`) if `size[0] * size[1] * 4 != rgba.len()`. It returns `Self`, i.e.
`ColorImage`, by value — *not* `Result`, *not* `Arc`.

Other verified constructors in 0.36.2:

```rust
pub fn new(size: [usize; 2], pixels: Vec<Color32>) -> Self          // line 61
pub fn filled(size: [usize; 2], color: Color32) -> Self             // line 75
pub fn from_rgba_unmultiplied(size: [usize; 2], rgba: &[u8]) -> Self // line 113
pub fn from_rgba_premultiplied(size: [usize; 2], rgba: &[u8]) -> Self // line 128
pub fn from_gray(size: [usize; 2], gray: &[u8]) -> Self             // line 146
pub fn from_gray_iter(size: [usize; 2], gray_iter: impl Iterator<Item = u8>) -> Self // 163
pub fn from_rgb(size: [usize; 2], rgb: &[u8]) -> Self               // line 193
pub fn as_raw(&self) -> &[u8]        // feature = "bytemuck"
pub fn as_raw_mut(&mut self) -> &mut [u8]
```

**`ColorImage::new` silently changed meaning.** I compared the published tarballs directly:

| | epaint 0.27.1 / 0.29.1 | epaint 0.36.2 |
|---|---|---|
| fields | `size`, `pixels` | `size`, **`source_size`**, `pixels` |
| `ColorImage::new` | `new(size: [usize; 2], color: Color32)` — makes a **filled** image | `new(size: [usize; 2], pixels: Vec<Color32>)` — takes **pixels** |
| fill helper | (that *was* `new`) | `ColorImage::filled(size, color)` |

So old code `ColorImage::new([w, h], Color32::RED)` is now a type error — good, it won't compile
silently — and struct-literal construction `ColorImage { size, pixels }` now fails because
`source_size` is missing. **Use `ColorImage::new(size, pixels)` or `ColorImage::filled(size, color)`.**
(This change is not mentioned in the epaint CHANGELOG at 0.36.2, so it was undocumented.)

`ColorImage::from_rgb` will **not** use premultiplied alpha, and `from_rgb` "is what you want to use
after having loaded an image file (and if you don't need the alpha channel)"; the doc comment on
`from_rgba_unmultiplied` explicitly says *"This is usually what you want to use after having loaded
an image file."*

### `Context::load_texture` → `TextureHandle`

`egui-0.36.2/src/context.rs:2390`:

```rust
pub fn load_texture(
    &self,
    name: impl Into<String>,
    image: impl Into<ImageData>,
    options: TextureOptions,
) -> TextureHandle
```

`ColorImage: Into<ImageData>` (the epaint enum `ImageData::{Color, Font}`), so a `ColorImage` goes
straight in. `TextureOptions::LINEAR` / `NEAREST` / `LINEAR_REPEAT` / `NEAREST_REPEAT` /
`LINEAR_MIRRORED_REPEAT` / `NEAREST_MIRRORED_REPEAT` are associated consts
(`epaint-0.36.2/src/textures.rs:183-223`).

`TextureHandle` is re-exported from epaint as `egui::TextureHandle`
(`pub use epaint::{… TextureHandle …}` at `egui-0.36.2/src/lib.rs:447-449`). Its methods
(`epaint-0.36.2/src/texture_handle.rs`):

```rust
pub fn id(&self) -> TextureId                                  // 64
pub fn set(&mut self, image: impl Into<ImageData>, options: TextureOptions)  // 70
pub fn set_partial(...)                                        // 78
pub fn size(&self) -> [usize; 2]                               // 90
pub fn size_vec2(&self) -> crate::Vec2                         // 98
```

Keep the `TextureHandle` alive for as long as you draw it — dropping it frees the GPU texture.

### Drawing it

Plain path, no `egui_extras` (from the `load_texture` doc example, `context.rs:2370-2385`):

```rust
let texture: egui::TextureHandle = ui.ctx().load_texture(
    "my_texture",
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba),
    egui::TextureOptions::LINEAR,
);

// Show the image:
ui.image((texture.id(), texture.size_vec2()));
```

`Ui::image` (`egui-0.36.2/src/ui.rs:2034`):

```rust
pub fn image<'a>(&mut self, source: impl Into<ImageSource<'a>>) -> Response
```

`ImageSource` (`egui-0.36.2/src/widgets/image.rs:571-609`):

```rust
pub enum ImageSource<'a> {
    Uri(Cow<'a, str>),
    Texture(SizedTexture),
    Bytes { uri: Cow<'static, str>, bytes: Bytes },
}
```

with these conversions (same file, lines 726-792):

```rust
impl<'a> From<&'a str>            for ImageSource<'a>
impl<'a> From<&'a String>         for ImageSource<'a>
impl      From<String>            for ImageSource<'static>
impl<'a> From<&'a Cow<'a, str>>   for ImageSource<'a>
impl<'a> From<Cow<'a, str>>       for ImageSource<'a>
impl<T: Into<Bytes>> From<(&'static str, T)> for ImageSource<'static>
impl<T: Into<Bytes>> From<(Cow<'static, str>, T)> for ImageSource<'static>
impl<T: Into<Bytes>> From<(String, T)> for ImageSource<'static>
impl<T: Into<SizedTexture>> From<T> for ImageSource<'static>   // line 790
```

and `impl<'a> From<&'a TextureHandle> for SizedTexture` (`egui-0.36.2/src/load.rs:489`) plus
`impl From<(TextureId, Vec2)> for SizedTexture` (`load.rs:482`).

> **Flagged as inferred, not verified:** because of the blanket impl at `image.rs:790`,
> `ui.image(&texture_handle)` and `egui::Image::new(&texture_handle)` *should* compile
> (`'a` unifies to `'static`). I could not run the compiler to confirm this. The form the official
> docs actually use, and which I am certain of, is `ui.image((texture.id(), texture.size_vec2()))`.
> `egui::Image::from_texture(texture: impl Into<SizedTexture>)` with `&texture` is certain — that is
> a direct `From` impl.

The `Image` widget API (`egui-0.36.2/src/widgets/image.rs`):

```rust
pub fn new(source: impl Into<ImageSource<'a>>) -> Self           // 64
pub fn from_uri(uri: impl Into<Cow<'a, str>>) -> Self            // 95
pub fn from_texture(texture: impl Into<SizedTexture>) -> Self    // 102
pub fn from_bytes(uri: impl Into<Cow<'static, str>>, bytes: impl Into<Bytes>) -> Self // 111
pub fn texture_options(mut self, texture_options: TextureOptions) -> Self // 120
pub fn max_width(mut self, width: f32) -> Self                   // 129
pub fn max_height(mut self, height: f32) -> Self                 // 138
pub fn max_size(mut self, size: Vec2) -> Self                    // 147
pub fn maintain_aspect_ratio(mut self, value: bool) -> Self      // 154
pub fn fit_to_original_size(mut self, scale: f32) -> Self        // 168
pub fn fit_to_exact_size(mut self, size: Vec2) -> Self           // 177
pub fn fit_to_fraction(mut self, fraction: Vec2) -> Self         // 186
pub fn shrink_to_fit(self) -> Self                               // 197
pub fn sense(mut self, sense: Sense) -> Self                     // 203
pub fn uv(mut self, uv: impl Into<Rect>) -> Self                 // 210
pub fn bg_fill(mut self, bg_fill: impl Into<Color32>) -> Self    // 217
pub fn tint(mut self, tint: impl Into<Color32>) -> Self          // 224
pub fn rotate(mut self, angle: f32, origin: Vec2) -> Self        // 239
pub fn corner_radius(mut self, corner_radius: impl Into<CornerRadius>) -> Self // 252
pub fn show_loading_spinner(mut self, show: bool) -> Self        // 264
pub fn alt_text(mut self, label: impl Into<String>) -> Self      // 273
```

Note `corner_radius` — **not** `rounding` (renamed in 0.31).

Full working album-art snippet:

```rust
struct Player { cover: Option<egui::TextureHandle> }

fn upload_cover(&mut self, ctx: &egui::Context, w: usize, h: usize, rgba: &[u8]) {
    let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba);
    self.cover = Some(ctx.load_texture("cover", color_image, egui::TextureOptions::LINEAR));
}

// in App::ui:
if let Some(tex) = &self.cover {
    ui.image((tex.id(), tex.size_vec2()));                              // certain
    ui.add(egui::Image::from_texture(tex).fit_to_exact_size(egui::vec2(128.0, 128.0))); // certain
}
```

---

## 5. `egui_extras` image loaders

The install call (`egui_extras-0.36.2/src/lib.rs:32`, implemented in `src/loaders.rs:58`):

```rust
pub use loaders::install_image_loaders;   // egui_extras::install_image_loaders
pub fn install_image_loaders(ctx: &egui::Context)
```

It takes `&egui::Context`. Since `Ui: Deref<Target = Context>`, you can also pass `ui` by deref.
Install it once, in your app constructor:

```rust
impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::default()
    }
}
```

It is idempotent — each loader is guarded by `if !ctx.is_loader_installed(…::ID)`.

### Cargo features — the non-obvious part

From `egui_extras-0.36.2/Cargo.toml`:

```toml
[features]
all_loaders = ["file", "http", "image", "svg", "gif", "webp"]
datepicker = ["jiff"]
default = ["dep:mime_guess2"]
file = ["dep:mime_guess2"]
gif = ["image", "image/gif"]
http = ["dep:ehttp"]
image = ["dep:image"]
serde = ["egui/serde", "enum-map/serde", "dep:serde"]
svg = ["resvg"]
svg_text = ["svg", "resvg/text", "resvg/system-fonts"]
syntect = ["dep:syntect"]
webp = ["image", "image/webp"]
```

and

```toml
[dependencies.image]
version = "0.25.6"
optional = true
default-features = false
```

The `image` feature only turns on `dep:image` — **it enables zero image formats.** The loader
filters by enabled decoders (`src/loaders/image_loader.rs`):

```rust
// Uses only the enabled image crate features
ImageFormat::from_extension(ext).is_some_and(|format| format.reading_enabled())
```

So `egui_extras = { version = "0.36.2", features = ["image", "file"] }` alone will **not** decode a
PNG. You must also enable the format features. Working dependency block for a music player
(album art: PNG/JPEG, plus GIF/WebP/SVG if you want them):

```toml
[dependencies]
eframe = "0.36.2"
egui = "0.36.2"

# `file` = read `file://` paths from disk; `image` = the ImageCrateLoader itself.
egui_extras = { version = "0.36.2", features = ["image", "file", "gif", "webp", "svg"] }

# egui_extras pulls `image` with default-features = false, so enable decoders explicitly:
image = { version = "0.25.6", default-features = false, features = ["png", "jpeg"] }
```

`all_loaders` (= `file`, `http`, `image`, `svg`, `gif`, `webp`) is the shortcut that turns on every
loader, but it still does **not** add `image/png` — you always need that line separately. Format
features you may want: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/bmp`,
`image/tiff`, `image/ico`, `image/avif`, plus `image/hdr`/`image/exr` for those.

Note also that `default = ["dep:mime_guess2"]` — if you use `default-features = false` you lose
mime guessing for `file://` URIs; keep defaults on.

Loader behaviour (doc comment on `install_image_loaders`):
- the `file` loader is `#[cfg(all(not(target_arch = "wasm32"), feature = "file"))]` — native only.
- the `image` loader "will attempt to load any URI with any extension other than `svg`. It will also
  try to load any URI without an extension."
- the `svg` loader needs an `svg` extension and will *not* load an extension-less URI.
- a `BytesPoll::Ready::mime` always takes precedence over the extension.

Usage:

```rust
ui.image("file://assets/cover.png");                     // from disk at runtime
ui.image(egui::include_image!("../assets/cover.png"));   // embedded at compile time
ui.add(egui::Image::new("file://assets/cover.png").max_width(200.0).corner_radius(10));
```

`egui::include_image!` is a macro in `egui-0.36.2/src/lib.rs:535` that produces an
`ImageSource::Bytes` (`bytes://…` URI). It is the most ergonomic option and needs no loaders
registered for the *bytes* step, but decoding still goes through the `image` loader, so the format
features above still apply.

`egui_extras` also re-exports `Size`, the `Table*` types (`pub use crate::table::*`), `Strip*`
types, `DatePickerButton` (feature `datepicker`) and the `syntax_highlighting` module.

---

## 6. Scrolling, panels, layout

### Scrollable list

`ScrollArea` (`egui-0.36.2/src/containers/scroll_area.rs`):

```rust
pub fn vertical() -> Self                                  // 375
pub fn horizontal() -> Self                                // 369
pub fn both() -> Self                                      // 381
pub fn new(direction_enabled: impl Into<Vec2b>) -> Self    // 394
pub fn max_height(mut self, max_height: f32) -> Self       // 432
pub fn id_salt(mut self, id_salt: impl AsIdSalt) -> Self   // 482
pub fn auto_shrink(mut self, auto_shrink: impl Into<Vec2b>) -> Self  // 605
pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> …  // 959
pub fn show_rows<R>(…)                                     // 983
pub fn show_viewport<R>(…)                                 // 1020
```

Idiomatic 0.36 track-list — `ScrollArea::vertical()` now takes a `&mut Ui` (like everything else):

```rust
egui::ScrollArea::vertical()
    .auto_shrink([false; 2])   // fill the panel instead of shrink-wrapping
    .show(ui, |ui| {
        for (i, track) in self.tracks.iter().enumerate() {
            if ui.selectable_label(self.selected == i, &track.title).clicked() {
                self.selected = i;
            }
        }
    });
```

For a long library, the virtualised form is much faster:

```rust
let row_height = ui.text_style_height(&egui::TextStyle::Body);
egui::ScrollArea::vertical().show_rows(ui, row_height, self.tracks.len(), |ui, row_range| {
    for i in row_range {
        ui.selectable_label(self.selected == i, &self.tracks[i].title);
    }
});
```

### Panels — `SidePanel` and `TopBottomPanel` are GONE

**This is the biggest layout break.** In 0.36.2 there is a single unified `egui::Panel` type.
Searching the whole `egui-0.36.2/src` tree for the identifiers `SidePanel` and `TopBottomPanel`
returns **zero matches**. Timeline from the changelogs:

- egui 0.34.0: *"As part of the above work, we have unified the panel API. `SidePanel` and
  `TopBottomPanel` are deprecated, replaced by a single `Panel`. Furthermore, it is now deprecated
  to use panels directly on `Context`. Use the `show_inside` functions instead, acting on `Ui`s."*
  (PRs #5659, #7781, #7783.)
- egui 0.35.0: *"Remove everything that was marked `#[deprecated]`"* (#8105) — this deleted the old
  types — plus *"Rename `Panel` methods"* (#8192).

`Panel` API (`egui-0.36.2/src/containers/panel.rs`):

```rust
pub struct Panel { … }                                    // 206

pub fn left(id: impl Into<Id>) -> Self                    // 249
pub fn right(id: impl Into<Id>) -> Self                   // 256
pub fn top(id: impl Into<Id>) -> Self                     // 265  (resizable(false) by default)
pub fn bottom(id: impl Into<Id>) -> Self                  // 274  (resizable(false) by default)

pub fn resizable(mut self, resizable: bool) -> Self       // 322
pub fn drag_to_open(mut self, drag_to_open: bool) -> Self  // 340  (new in 0.36)
pub fn show_separator_line(mut self, show_separator_line: bool) -> Self // 362
pub fn default_size(mut self, default_size: f32) -> Self  // 369
pub fn min_size(mut self, min_size: f32) -> Self          // 380
pub fn max_size(mut self, max_size: f32) -> Self          // 387
pub fn size_range(mut self, size_range: impl Into<Rangef>) -> Self  // 394
pub fn exact_size(mut self, size: f32) -> Self            // 405
pub fn frame(mut self, frame: Frame) -> Self              // 413

pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>  // 422
pub fn show_collapsible<R>(…)                             // 451
pub fn show_switched<R>(…)                                // 563
```

Defaults from `Panel::new` (`panel.rs:281-305`): `resizable: true`, `drag_to_open: true`,
`show_separator_line: true`; for left/right `default_outer_size = Some(200.0)` and
`outer_size_range = 96.0..=f32::INFINITY`; for top/bottom `default_outer_size = None` and
`20.0..=f32::INFINITY`.

Two deprecation aliases survive in 0.36.2: `show_inside` is `#[deprecated = "Renamed to `show`"]`
(lines 427 and 1217), as are `show_animated_inside` → `show_collapsible` and
`show_animated_between_inside` → `show_switched`. Use the non-deprecated names.

So your music player shell is:

```rust
impl eframe::App for MusicPlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Left: library list, user-resizable.
        egui::Panel::left("library")
            .resizable(true)
            .default_size(240.0)
            .min_size(120.0)
            .show(ui, |ui| {
                ui.heading("Library");
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| { /* … */ });
            });

        // Right panel, if you want one.
        egui::Panel::right("queue").show(ui, |ui| { ui.label("Queue"); });

        // Top: menu / search bar.
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| { ui.text_edit_singleline(&mut self.filter); });
        });

        // Bottom: transport controls. Non-resizable, as you asked.
        egui::Panel::bottom("player").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Play").clicked() { /* … */ }
                ui.label(format!("Now playing: {}", self.tracks[self.selected].title));
            });
        });

        // Everything else.
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Now Playing");
        });
    }
}
```

Note: `Panel::top`/`Panel::bottom` are already `resizable(false)`; calling `.resizable(false)`
explicitly as above is harmless and self-documenting. Also note the doc remark on `resizable`:
*"If you want your panel to be resizable you also need to make the ui use the available space"* —
use a `ScrollArea`, `Ui::take_available_space()`, or similar, otherwise the panel has nothing to
grow into.

### `CentralPanel`

```rust
#[derive(Default)]
pub struct CentralPanel { … }        // panel.rs:1186-1189

pub fn no_frame() -> Self             // 1193 — no margin, no background
pub fn default_margins() -> Self       // 1200
pub fn frame(mut self, frame: Frame) -> Self  // 1206
pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>  // 1212
```

`CentralPanel::default()` works (Default is derived) and gives you a background + margins.
`CentralPanel::show` also takes `&mut Ui` now, and `show_inside` is the deprecated alias.

### Selectable rows

`ui.selectable_label` **is not deprecated** in 0.36.2 — it is the recommended API
(`egui-0.36.2/src/ui.rs:1925-1931`):

```rust
/// Show a label which can be selected or not.
///
/// See also [`Button::selectable`] and [`Self::toggle_value`].
#[must_use = "You should check if the user clicked this with `if ui.selectable_label(…).clicked() { … } "]
pub fn selectable_label<'a>(&mut self, checked: bool, text: impl IntoAtoms<'a>) -> Response {
    Button::selectable(checked, text).ui(self)
}
```

Two things changed vs 0.27–0.29:

1. The second parameter is now `impl IntoAtoms<'a>`, **not** `impl Into<WidgetText>`. `&str`,
   `String`, `RichText` etc. all still work, but `IntoAtoms` is the 0.35+ "atoms" API.
2. It is `#[must_use]` — ignoring the returned `Response` now produces a warning.

The `SelectableLabel` **widget struct no longer exists** as a public type in 0.36.2. The only
remaining occurrences of the identifier are the `WidgetType::SelectableLabel` *enum variant*
(`data/output.rs:756`, `response.rs:939`) and its re-export in the `WidgetType` enum
(`lib.rs:640`). If you were doing `ui.add(egui::SelectableLabel::new(sel, "x"))`, replace it with
`ui.selectable_label(sel, "x")` or `ui.add(egui::Button::selectable(sel, "x"))`.

Also available: `ui.selectable_value(&mut value, alternative, text)` for enum/`PartialEq` state
(`ui.rs:1939`):

```rust
pub fn selectable_value<'a, Value: PartialEq>(
    &mut self,
    current_value: &mut Value,
    selected_value: Value,
    text: impl IntoAtoms<'a>,
) -> Response
```

A typical library row:

```rust
let selected = self.selected == i;
if ui.selectable_label(selected, format!("{} — {}", track.artist, track.title)).clicked() {
    self.selected = i;
}
```

### Track list as a table (`egui_extras::TableBuilder`)

For a music player a resizable-column table is usually what you want, and `egui_extras` ships one
(`egui_extras-0.36.2/src/table.rs`; re-exported at the crate root via `pub use crate::table::*`
plus `pub use crate::sizing::Size`).

`Column` constructors and builders (`table.rs:47-155`):

```rust
pub fn auto() -> Self                                        // 53
pub fn auto_with_initial_suggestion(suggested_width: f32) -> Self  // 62
pub fn initial(width: f32) -> Self                           // 67
pub fn exact(width: f32) -> Self                             // 72  (never shrink/grow, clips)
pub fn remainder() -> Self                                   // 83  (share leftover space)
pub fn resizable(mut self, resizable: bool) -> Self          // 102
pub fn clip(mut self, clip: bool) -> Self                    // 116 (default false)
pub fn at_least(mut self, minimum: f32) -> Self              // 125
pub fn at_most(mut self, maximum: f32) -> Self               // 134
pub fn range(mut self, range: impl Into<Rangef>) -> Self     // 141
pub fn auto_size_this_frame(mut self, auto_size_this_frame: bool) -> Self  // 150
```

`TableBuilder` (`table.rs:258-585`):

```rust
pub fn new(ui: &'a mut Ui) -> Self                           // 259
pub fn id_salt(mut self, id_salt: impl AsIdSalt) -> Self     // 277
pub fn striped(mut self, striped: bool) -> Self              // 286
pub fn sense(mut self, sense: egui::Sense) -> Self           // 293
pub fn resizable(mut self, resizable: bool) -> Self          // 309 (default false)
pub fn vscroll(mut self, vscroll: bool) -> Self              // 316
pub fn scroll_to_row(mut self, row: usize, align: Option<Align>) -> Self  // 349
pub fn min_scrolled_height(mut self, min_scrolled_height: f32) -> Self    // 370
pub fn max_scroll_height(mut self, max_scroll_height: f32) -> Self        // 380
pub fn auto_shrink(mut self, auto_shrink: impl Into<Vec2b>) -> Self       // 393
pub fn animate_scrolling(mut self, animated: bool) -> Self   // 411
pub fn cell_layout(mut self, cell_layout: egui::Layout) -> Self           // 418
pub fn column(mut self, column: Column) -> Self              // 425
pub fn columns(mut self, column: Column, count: usize) -> Self            // 432
pub fn header(self, height: f32, add_header_row: impl FnOnce(TableRow<'_, '_>)) -> Table<'a>  // 452
// then on Table:
pub fn body<F>(self, add_body_contents: F) -> ScrollAreaOutput<()>        // 527 / 704
```

`TableBody::row(&mut self, height: f32, add_row_content: impl FnOnce(TableRow<'a, '_>))` (976),
`TableRow::col(&mut self, add_cell_contents: impl FnOnce(&mut Ui)) -> (Rect, Response)` (1274), and
`TableRow::set_selected(&mut self, selected: bool)` (1329).

```rust
use egui_extras::{Column, TableBuilder};

TableBuilder::new(ui)
    .id_salt("tracks")
    .striped(true)
    .resizable(true)
    .sense(egui::Sense::click())
    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
    .column(Column::auto())                       // #
    .column(Column::initial(180.0).at_least(80.0).clip(true))  // title
    .column(Column::initial(140.0).clip(true))    // artist
    .column(Column::remainder())                  // album
    .column(Column::exact(56.0))                  // duration
    .min_scrolled_height(0.0)
    .header(20.0, |mut header| {
        header.col(|ui| { ui.strong("#"); });
        header.col(|ui| { ui.strong("Title"); });
        header.col(|ui| { ui.strong("Artist"); });
        header.col(|ui| { ui.strong("Album"); });
        header.col(|ui| { ui.strong("Time"); });
    })
    .body(|mut body| {
        for (i, track) in self.tracks.iter().enumerate() {
            let row_height = 18.0;
            body.row(row_height, |mut row| {
                row.set_selected(self.selected == i);
                row.col(|ui| { ui.label(format!("{}", i + 1)); });
                row.col(|ui| { ui.label(&track.title); });
                row.col(|ui| { ui.label(&track.artist); });
                row.col(|ui| { ui.label(&track.album); });
                row.col(|ui| { ui.label(&track.duration); });
                if row.response().clicked() {
                    self.selected = i;
                }
            });
        }
    });
```

`TableRow` also has `pub fn response(&self) -> Response` (1349), `pub fn index(&self) -> usize` (1357)
and `pub fn col_index(&self) -> usize` (1363) — so the click check above
(`row.response().clicked()`) is the verified way to select a row.

For thousands of tracks, swap the `body` loop for the virtualised variants (`table.rs:1027`, `1112`):

```rust
pub fn rows(
    mut self,
    row_height_sans_spacing: f32,
    total_rows: usize,
    add_row_content: impl FnMut(TableRow<'_, '_>),
)

pub fn heterogeneous_rows(
    mut self,
    heights: impl Iterator<Item = f32>,
    add_row_content: impl FnMut(TableRow<'_, '_>),
)
```

Note `row_height_sans_spacing` — item spacing is added internally, so pass the text height, not
`text_height + spacing.y`. The `Column::auto` doc note warns that with these variants auto-sizing
"will only be based on the currently visible rows".

---

## 7. Notable breaking changes vs 0.27–0.29

Ordered by how likely they are to bite a music player port.

| # | Change | Since | What breaks |
|---|---|---|---|
| 1 | **`App::update` removed; required method is now `App::ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`** | deprecated 0.34 (#7775), deleted 0.35 (#8105) | Every `impl eframe::App`. You now get a `&mut Ui`, not a `&Context`. `App::logic(&mut self, ctx, frame)` is the new home for non-painting per-frame work (it also runs while the window is hidden). |
| 2 | **`SidePanel` / `TopBottomPanel` deleted**, replaced by `egui::Panel::{left,right,top,bottom}(id)` | deprecated 0.34 (#5659), deleted 0.35 (#8105) | Every panel-based layout. Also `Panel::default_width` → `default_size`, `width_range` → `size_range`. |
| 3 | **Panels are shown into a `Ui`, not a `Context`.** `Panel::show(ui, …)`, `CentralPanel::show(ui, …)` | 0.34 (#7781/#7783), renamed to `show` in 0.35 (#8192) | `panel.show(ctx, …)` no longer exists. `show_inside(ui, …)` still exists but is `#[deprecated = "Renamed to `show`"]`. |
| 4 | **`ColorImage` gained a third field `source_size: Vec2`, and `ColorImage::new(size, color)` became `ColorImage::new(size, pixels)`** | between 0.29 and 0.36 (verified by diffing published 0.27.1/0.29.1 vs 0.36.2; undocumented in the epaint CHANGELOG) | `ColorImage { size, pixels }` literals fail; `ColorImage::new([w,h], Color32::RED)` is now a type error — use `ColorImage::filled(size, color)`. `from_rgba_unmultiplied` is unchanged. |
| 5 | `Rounding` renamed to `CornerRadius`; `Margin`/`CornerRadius`/`Shadow` narrowed to `i8`/`u8` | 0.31 (#5673, #5563/#5567/#5568) | `Image::rounding(…)` → `Image::corner_radius(…)`; float literals into margin/radius fields need care (`CornerRadiusF32` exists for `f32`). |
| 6 | `Ui` now `Deref<Target = Context>`; `Context::run` → `Context::run_ui`; viewports get a `&mut Ui` | 0.34 (#7770, #7736, #7779) | Custom integrations that called `ctx.run(raw_input, …)`. App code can now write `ui.input(…)` instead of `ui.ctx().input(…)`. |
| 7 | `Context::style` renamed to `global_style`; `Context::used_size` / `available_rect` deprecated | 0.34 (#7772, #7788) | `ctx.style()` → `ctx.global_style()`; the `CreationContext` doc comment says `cc.egui_ctx.set_fonts` and `cc.egui_ctx.set_global_style`. |
| 8 | `Modifiers` removed from `RawInput` and made an `egui::Event`; `clip_rect_margin` removed | 0.36 (#8336, #8366) | Code reading `raw_input.modifiers` in `App::raw_input_hook`. |
| 9 | Font stack switched `ab_glyph` → `skrifa` + `vello_cpu`, with font hinting and variations | 0.34 (#7694, #7859) | Custom `FontData` / `FontTweak` code; text metrics shift slightly. |
| 10 | `impl Into<f32>` arguments removed | 0.35 (#8194) | Calls that relied on implicit integer→float coercion in those arguments now need explicit floats. |
| 11 | `Panel` method renames + drag-to-close/reopen panels, `Panel` never overflows width | 0.35 (#8182, #8192, #8198) | Cosmetic/layout tuning. |
| 12 | MSRV raised: `rust-version = "1.95"` for `egui`, `eframe` and `egui_extras` 0.36.2 | 0.36 (#8348: 1.92 → 1.95) | Your 1.99 toolchain is fine. |

Things that did **not** change and are still valid from the 0.27–0.29 era:

- `eframe::run_native(name, options, Box::new(|cc| Ok(Box::new(App::new(cc)))))` and its
  `-> eframe::Result` (`Result<(), eframe::Error>`) return type — the `Ok(...)` wrapper dates from
  0.28, so if your code is 0.29-era it already has it.
- `NativeOptions { viewport: egui::ViewportBuilder::default()…, ..Default::default() }` — unchanged
  since 0.24 (#3572).
- `Context::load_texture(name, image, options) -> TextureHandle` and `TextureOptions::LINEAR`.
- `ColorImage::from_rgba_unmultiplied([w, h], &rgba)` — byte-identical signature since at least 0.27.1.
- `egui_extras::install_image_loaders(&ctx)` and `ui.image("file://…")`.
- `ScrollArea::vertical()`, `.auto_shrink(…)`, `.show(ui, …)`, `.show_rows(…)`.
- `egui::ViewportCommand::{Title, InnerSize, Close, Icon}` and
  `Frame::storage()` / `Frame::storage_mut()` / `Frame::info()` / `Frame::winit_window()`.

`eframe::Frame` itself (the *eframe* one, not `egui::Frame`) still exists with `is_web()`, `info()`,
`storage()`, `storage_mut()`, `winit_window()`, `gl()`, `wgpu_render_state()`,
`wgpu_surface_config()`, `set_wgpu_surface_config()` — and `assert_not_impl_any!(Frame: Clone)`, so
it is deliberately not `Clone`.

Separately, **`egui::Frame`** (the visual container, a different type) lost `Frame::none()`; it now
has `Frame::NONE` (const), `Frame::new()`, and style-taking constructors
`group(&Style)`, `side_top_panel(&Style)`, `central_panel(&Style)`, `window(&Style)`, `menu(&Style)`,
`popup(&Style)`, `canvas(&Style)`, `dark_canvas(&Style)` — plus `inner_margin`, `outer_margin`,
`fill`, `stroke`, `corner_radius`, `shadow`, `multiply_with_opacity` and
`show(ui, add_contents)`.

---

## 8. Full skeleton

See `skeleton_0_36_2.rs` next to this file. It exercises `run_native`, `App::ui`, `App::logic`,
`NativeOptions` + `ViewportBuilder`, `IconData` from raw RGBA and from PNG, `ColorImage`,
`load_texture`, `ui.image`, `egui::Image`, `egui_extras::install_image_loaders`, `ScrollArea`,
`Panel::left/right/top/bottom`, `CentralPanel`, and `selectable_label` / `selectable_value`.

**It has not been compiled** (no linker on this machine) — treat it as source-verified, not
compiler-verified.

## 9. What I could NOT verify

1. **Anything by compilation.** No `link.exe`, no Windows SDK. `cargo check` fails on build scripts
   and proc-macros. A dependency fetch did succeed, so the tree is fine — only linking is missing.
   Consequence: I cannot promise zero compile errors, only that every signature matches the
   published source.
2. **`ui.image(&texture_handle)` / `egui::Image::new(&texture_handle)`** — inferred from the blanket
   `impl<T: Into<SizedTexture>> From<T> for ImageSource<'static>` (`image.rs:790`) plus
   `impl<'a> From<&'a TextureHandle> for SizedTexture` (`load.rs:489`). Very likely fine; not
   compiler-confirmed. The certain alternative is `ui.image((tex.id(), tex.size_vec2()))`.
3. **`IntoAtoms` conversions.** `selectable_label`/`button` now take `impl IntoAtoms<'a>`. I did not
   enumerate which concrete types implement `IntoAtoms`; I confirmed the `&str`/`String` examples
   used in the 0.36.2 docs and examples compile-equivalent usage, but exotic types may not.
4. **Exact `Vec2b`/`Rangef`/`Margin` coercion rules** for `auto_shrink`, `size_range`, `inner_margin`
   — I read the parameter types (`impl Into<Vec2b>`, `impl Into<Rangef>`, `impl Into<Margin>`) but
   did not verify every accepted input type.
5. **Which 0.30–0.33 release changed `ColorImage::new`.** I confirmed 0.27.1 and 0.29.1 use
   `new(size, color)` and 0.36.2 uses `new(size, pixels)`, but did not bisect the intermediate
   versions, and the epaint CHANGELOG does not mention it.
6. ~~`egui_extras` `TableBuilder` API details~~ — **now covered in §6** (`Column`, `TableBuilder`,
   `TableBody`, `TableRow` signatures all read from `egui_extras-0.36.2/src/table.rs`). Still
   unverified: `TableState` (line 585) and `TableScrollOptions` (line 194) internals, and the `Size`
   type from `egui_extras::sizing`.
7. **`ViewportBuilder::with_monitor`** signature — it is listed in the 0.35 changelog (#8140) but I
   did not read its exact signature.
8. The docs.rs HTML I fetched was **truncated** ("Content truncated" / "Omitted 50620 bytes"), so for
   the `egui` and `eframe` changelog pages I relied on the raw GitHub markdown plus the local crate
   sources rather than the full rendered pages.
