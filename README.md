# Echo —— 本地音乐播放器

一个用 Rust 写的本地音乐播放器，技术栈为 **egui + Rodio + Lofty + SQLite**。

- 🎵 **音乐库管理** —— 添加文件夹、后台扫描入库、增量重扫、搜索过滤
- ▶️ **点击即播** —— 曲库列表单击直接播放，支持上一首/下一首/进度拖动/音量
- 🖼️ **封面展示** —— 读取音频文件内嵌的专辑封面，列表显示缩略图、播放区显示大图
- 🀄 **中文友好** —— 自动加载系统中文字体，中文标签与界面不会显示成方块
- 🌍 **按平台分层** —— 平台差异收敛在一个模块里，Linux/macOS 由 CI 矩阵验证

---

## 快速开始

```bash
# 1. 准备构建环境（首次需要，幂等可重复执行）
pwsh -File toolchain/bootstrap.ps1

# 2. 构建与运行
cargo build --release
./target/release/echo.exe

# 3. 跑测试
cargo test
```

首次运行时，点击左上角 **「添加文件夹」** 选择存放音乐的目录，等待扫描完成即可。

---

## 关于构建环境（请先读这一节）

**如果本机已经装了 Visual Studio / MSVC 构建工具**，直接 `cargo build --release` 就行，
`toolchain/bootstrap.ps1` 会检测到并立刻退出，不做任何事。

**如果没装**（本项目的开发机就是这种情况），标准 `cargo build` 会因为找不到
`link.exe` 而失败。此时代码本身没问题，缺的是工具链。`bootstrap.ps1` 会用
**完全不需要管理员权限**的方式补齐：

| 缺口 | 解决办法 |
|---|---|
| 没有链接器 `link.exe` | 用 Rust 自带的 **`rust-lld`**（即 lld-link，随工具链分发） |
| 没有 C 编译器 | 下载 **LLVM clang-cl**（免安装压缩包，解压即用） |
| 没有 MSVC CRT / Windows SDK | 用 **`xwin`** 从微软官方渠道拉取库与头文件 |

所有产物都放在仓库内的 `.tools/` 目录（已在 `.gitignore` 中），
不动系统环境、不改注册表、不需要管理员权限。

相关配置集中在 `.cargo/config.toml`，里面有详细注释说明每一项的由来与实测结论。

### 为什么必须用 clang-cl 而不是 clang

`cc-rs` 在为 msvc 目标编译 C 代码时会传 `/arch:AVX512` 这类 MSVC 风格参数，
GNU 风格的 `clang` 驱动不认（实测报 `no such file or directory: '/arch:AVX512'`），
而 `clang-cl` 完全兼容。

### 关于 blake3 的 `pure` 特性

`blake3` 默认会编译 AVX-512 汇编，而 MSVC 目标下的汇编器 `ml64.exe` 只随
Visual Studio 分发。我们用官方的 `pure` 特性改走纯 Rust 实现 —— 封面哈希
不是性能瓶颈，用这点性能换取「零额外工具依赖」是划算的。

### GNU 备选链路

若 MSVC 链路因故不可用，仓库里还留了 GNU 工具链（w64devkit）的配置：

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
```

注意 `bootstrap.ps1` 会生成一个空的 `libgcc_eh.a`：GCC 13 之后不再附带它，
而 rustc 的 `windows-gnu` spec 仍硬编码 `-lgcc_eh`，没有这个空壳库就链接不过。

---

## 图形后端（Windows 上的一个坑）

eframe 默认让 wgpu 自选后端，它在 Intel 核显上会优先选 Vulkan。
本机的 Intel Vulkan 驱动（`igvk64.dll` 31.0.101.2141）在创建交换链时
**直接崩溃**：

```text
应用程序错误：echo.exe，故障模块 igvk64.dll，异常代码 0xc0000005
```

实测 DX12 与 OpenGL 后端都能稳定运行，因此程序在 Windows 上会默认改用
DX12（见 `src/lib.rs` 的 `prepare_graphics_backend`）。

这段兜底**完全尊重用户设置**：只要你已经设置了 `WGPU_BACKEND`
（eframe/wgpu 官方支持的环境变量），程序不会覆盖它。如果哪天驱动修好了，
或者你想换个后端：

```powershell
$env:WGPU_BACKEND = "vulkan"   # 或 dx12 / opengl / metal
./target/release/echo.exe
```

---

## 跨平台

代码本身是跨平台的，平台差异**只出现在 `src/platform/` 一个目录**里
（外加 `src/ui/fonts.rs` 需要按平台挑选字体文件）。

| 平台 | 状态 | 说明 |
|---|---|---|
| Windows | ✅ 已在本机验证 | 用 WASAPI 输出 |
| Linux | ✅ CI 验证 | 用 ALSA；**编译期需要 `libasound2-dev`** |
| macOS | ✅ CI 验证 | 用 CoreAudio |

在 Linux 上：

```bash
sudo apt-get install -y libasound2-dev   # rodio 的编译期硬依赖
cargo build --release
```

`libasound2-dev` 是 rodio 自身的要求，属于**架构上无法规避**的依赖，
已写入 CI 与本文档。

跨平台是否成立由 [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
的三平台矩阵给出确凿结论 —— 开发机只验证 Windows。

---

## 架构

分层原则：**核心逻辑零平台判定**，平台差异只存在于最里层。

```
src/
├── main.rs          组装入口
├── domain.rs        ★ 纯数据与纯逻辑（不依赖 egui/rodio/lofty/rusqlite）
├── db.rs            ★ SQLite 存储
├── metadata.rs      ★ 标签与封面读取（lofty）
├── library.rs       ★ 后台扫描（walkdir + rayon）
├── covers.rs        ★ 封面解码与纹理缓存
├── playback/        ◆ 播放引擎（rodio 内部选后端，本层 API 平台无关）
├── platform/        ◆◆ 唯一出现操作系统差异的地方
│   ├── mod.rs         门面与跨平台字体解析
│   ├── windows.rs     %WINDIR%\Fonts
│   ├── linux.rs       /usr/share/fonts 等
│   └── macos.rs       /System/Library/Fonts
└── ui/
    ├── app.rs        eframe::App 实现
    ├── fonts.rs      ◆ 中文字体装配
    └── widgets.rs    复用组件
```

`domain.rs` 是整个项目的稳定契约：它不依赖任何 GUI / 音频 / 数据库库，
因此换 GUI 框架或换播放引擎都只需替换外层实现。播放队列的
上一首/下一首/循环逻辑全在这里，并且有完整的单元测试。

新增一个平台支持，原则上只需在 `platform/` 下加一个子模块。

---

## 设计要点

**扫描很快。** 用 `(文件大小, 修改时间)` 判断文件是否变更，未变更的文件
连标签都不解析。扫描在独立线程里跑，进度通过 channel 上报，界面每帧
非阻塞消费 —— 扫描上万个文件也不会卡住界面。

**不会重复入库。** `path` 上有 `UNIQUE` 约束，写入用
`INSERT ... ON CONFLICT(path) DO UPDATE`。跳过的文件会同步刷新其
「本次已见到」标记，否则收尾的清理步骤会把它们误删。

**封面只存一份。** 封面按 blake3 内容哈希寻址，同一张专辑图在整个库里
只占一份空间。显示时按哈希缓存纹理，同一张图解码一次。

**损坏文件不会丢。** 无法解析的文件仍会在曲库里留下记录（而非被当成
「已消失」删除），并计入扫描错误数，用户能看到它们。

**没有声卡也能用。** 打不开音频设备时（远程桌面、容器）会降级为
「可浏览曲库但不可播放」，并在状态栏提示，而不是崩溃。

---

## 已知限制

- **Opus / WMA 只能看，不能播。** lofty 能读它们的标签，但 rodio 0.22
  无法解码这些编码。这类文件会出现在曲库中并标注「不可播放」。
  可播放格式：MP3 / FLAC / WAV / M4A-AAC / OGG-Vorbis。
- 封面只读**文件内嵌**的图，不会联网抓取，也不会读取同目录的 `cover.jpg`。
- 单曲循环下按「下一首」会正常换歌（只有自然播完才原地重播）——
  这是刻意的：否则无法跳过一首不喜欢的歌。

---

## 许可

MIT
