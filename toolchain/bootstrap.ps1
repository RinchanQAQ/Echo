# Echo 构建环境自举脚本（幂等，可重复执行）
#
# ============================================================================
# 为什么需要它
# ============================================================================
# 本机没有安装 Visual Studio / MSVC 构建工具：link.exe 不存在，也没有 C 编译器。
# 直接 `cargo build` 会失败。本脚本用**免管理员**的方式补齐标准
# x86_64-pc-windows-msvc 目标的构建能力：
#
#   1. 链接   —— Rust 自带的 rust-lld（即 lld-link，不需要 link.exe）；
#   2. 编译 C —— LLVM clang-cl（libsqlite3-sys 的 bundled SQLite 需要）；
#   3. 头文件与导入库 —— 用 xwin 从微软官方渠道拉取 MSVC CRT + Windows SDK。
#
# 全部落在仓库内的 .tools/ 目录（已在 .gitignore 中），不动系统环境、
# 不改注册表、不需要管理员权限。
#
# 在已有完整 MSVC 的机器或 CI 上，这些步骤都不需要，直接 `cargo build` 即可。
#
# 用法：
#   pwsh -File toolchain/bootstrap.ps1
#   cargo build --release
# ============================================================================

[CmdletBinding()]
param(
    # 只准备 MSVC 链路，跳过 GNU 备选链路（省下约 64 MB 下载）。
    [switch]$SkipGnuFallback
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ToolsDir = Join-Path $RepoRoot '.tools'
$SdkDir   = Join-Path $ToolsDir 'msvc-sdk'
$CacheDir = Join-Path $ToolsDir 'xwin-cache'
$ToolsBin = Join-Path $ToolsDir 'cargo-tools'
$XwinExe  = Join-Path $ToolsBin 'bin\xwin.exe'
$ClangDir = Join-Path $ToolsDir 'clang'
$ClangCl  = Join-Path $ClangDir 'bin\clang-cl.exe'
$LlvmLib  = Join-Path $ClangDir 'bin\llvm-lib.exe'

function Write-Step([string]$m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Write-Ok([string]$m)   { Write-Host "    OK  $m" -ForegroundColor Green }
function Write-Note([string]$m) { Write-Host "    --  $m" -ForegroundColor DarkGray }

# ---------------------------------------------------------------- 平台判定
$IsWindowsHost = $IsWindows -or ($PSVersionTable.PSEdition -eq 'Desktop')
if (-not $IsWindowsHost) {
    Write-Host @"
非 Windows 平台无需本脚本。标准构建步骤：

  rustup default stable
  cargo build
  cargo test

Linux 额外需要 rodio 的 ALSA 开发库（编译期硬依赖，无法避免）：
  sudo apt-get install -y libasound2-dev

macOS 开箱可用。
"@ -ForegroundColor Yellow
    exit 0
}

# 已经有完整 MSVC 的话，什么都不用做。
if ($env:VCINSTALLDIR -and (Get-Command link.exe -ErrorAction SilentlyContinue)) {
    Write-Ok '检测到已安装的 MSVC 构建工具，无需自举。直接 cargo build 即可。'
    exit 0
}

Write-Host 'Echo 构建环境自举' -ForegroundColor White

New-Item -ItemType Directory -Force -Path $ToolsDir | Out-Null

# ---------------------------------------------------------------- 1. Rust 工具链
Write-Step '检查 Rust 工具链'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    throw '未找到 rustup。请先从 https://rustup.rs 安装 Rust。'
}

$msvcToolchain = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-msvc'
if (-not (Test-Path $msvcToolchain)) {
    Write-Host '    安装 stable-x86_64-pc-windows-msvc …'
    rustup toolchain install stable-x86_64-pc-windows-msvc
}
Write-Ok 'stable-x86_64-pc-windows-msvc 可用'

# rust-lld 随工具链分发，位于 rustlib 下的 bin 目录。
$rustLld = Join-Path $msvcToolchain 'lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
if (-not (Test-Path $rustLld)) {
    throw "未找到 rust-lld.exe（预期位置：$rustLld）。请重新安装该工具链。"
}
Write-Ok 'rust-lld 就绪（作为链接器，无需 link.exe）'

# ---------------------------------------------------------------- 2. xwin
Write-Step '准备 xwin（免管理员拉取 MSVC CRT + Windows SDK）'
if (Test-Path $XwinExe) {
    Write-Ok "xwin 已安装：$XwinExe"
} else {
    # xwin 本身需要先被编译出来。此时 MSVC 链路还没就绪，
    # 所以用 GNU 工具链编译它（它依赖若干 C 库绑定）。
    $gnuBin = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin'
    if (-not (Test-Path $gnuBin)) {
        Write-Host '    安装 GNU 工具链以编译 xwin …'
        rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
    }
    Write-Host '    正在编译 xwin（首次约 3 分钟）…'
    $oldPath = $env:Path
    $env:Path = "$gnuBin;$oldPath"
    $env:CARGO_TARGET_DIR = Join-Path $ToolsDir 'build-tools'
    & cargo install xwin --locked --root $ToolsBin
    $env:Path = $oldPath
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $XwinExe)) {
        throw 'xwin 编译安装失败。'
    }
    Write-Ok "xwin 安装完成"
}

# ---------------------------------------------------------------- 3. MSVC SDK
$umLibDir   = Join-Path $SdkDir 'sdk\lib\um\x86_64'
$ucrtLibDir = Join-Path $SdkDir 'sdk\lib\ucrt\x86_64'
$crtLibDir  = Join-Path $SdkDir 'crt\lib\x86_64'
$ucrtIncDir = Join-Path $SdkDir 'sdk\include\ucrt'

function Test-SdkReady {
    return (Test-Path (Join-Path $umLibDir 'kernel32.lib')) -and
           (Test-Path (Join-Path $crtLibDir 'libcmt.lib')) -and
           (Test-Path (Join-Path $ucrtLibDir 'ucrt.lib')) -and
           (Test-Path (Join-Path $ucrtIncDir 'stdio.h')) -and
           (Test-Path (Join-Path $SdkDir 'crt\include\vcruntime.h'))
}

Write-Step '检查 MSVC CRT / Windows SDK（库 + 头文件）'
if (Test-SdkReady) {
    Write-Ok 'MSVC SDK 已就绪'
} else {
    Write-Host '    从微软官方源下载并展开（约 1.7 GB，首次约 1-2 分钟）…'
    $env:XWIN_ACCEPT_LICENSE = '1'
    # --cache-dir / --accept-license 是**顶层**选项，必须写在子命令之前。
    # --disable-symlinks：无管理员权限时无法创建符号链接；Rust 链接与
    # 编译都不需要它们，头文件目录在解包位置就能直接使用。
    & $XwinExe --cache-dir $CacheDir --accept-license splat --output $SdkDir --disable-symlinks
    # xwin 在无法创建符号链接时会返回非 0，但库与头文件其实已经完整落盘，
    # 因此这里以「关键文件是否齐全」判定成功，而不是看退出码。
    if (-not (Test-SdkReady)) {
        throw "MSVC SDK 准备失败：关键文件不完整。请检查 $SdkDir"
    }
    Write-Ok 'MSVC SDK 准备完成（符号链接相关警告可忽略）'
}

# ---------------------------------------------------------------- 4. clang-cl
Write-Step '准备 C 编译器（clang-cl）'
if (Test-Path $ClangCl) {
    Write-Ok 'clang 已安装'
} else {
    Write-Host '    下载 LLVM（约 860 MB）…'
    $url = 'https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.3/clang+llvm-23.1.3-x86_64-pc-windows-msvc.tar.xz'
    $archive = Join-Path $ToolsDir 'clang.tar.xz'
    $ok = $false
    for ($i = 1; $i -le 3 -and -not $ok; $i++) {
        try {
            Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing -TimeoutSec 1800
            $ok = $true
        } catch {
            Write-Note "第 $i 次下载失败：$($_.Exception.Message)"
            Start-Sleep -Seconds 5
        }
    }
    if (-not $ok) { throw 'LLVM 下载失败。' }

    Write-Host '    解压…'
    # 注意：Windows 自带的 tar.exe 不支持 .tar.xz（没有 xz 解码器）。
    # 因此用 tar + Python 的 lzma 组合解压。
    $py = 'C:\Users\123\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe'
    if (-not (Test-Path $py)) { $py = (Get-Command python -ErrorAction SilentlyContinue).Source }
    if (-not $py) { throw '需要 Python 3（含 lzma 模块）来解压 .tar.xz。' }

    $script = Join-Path $ToolsDir 'extract.py'
    @'
import tarfile, sys
src, dst = sys.argv[1], sys.argv[2]
with tarfile.open(src, "r:xz") as tf:
    tf.extractall(dst, filter="data")
'@ | Set-Content $script -Encoding utf8
    & $py $script $archive $ToolsDir
    if ($LASTEXITCODE -ne 0) { throw 'LLVM 解压失败。' }

    # 发布包会解出一个带版本号的目录，改名成稳定的 clang/。
    Get-ChildItem $ToolsDir -Directory -Filter 'clang+llvm-*' | ForEach-Object {
        if (-not (Test-Path $ClangDir)) { Rename-Item $_.FullName 'clang' }
    }
    Remove-Item $archive -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $ClangCl)) { throw "LLVM 安装失败：未找到 $ClangCl" }
    Write-Ok 'clang-cl 就绪'
}

if (-not (Test-Path $LlvmLib)) {
    throw "未找到 llvm-lib.exe（预期位置：$LlvmLib），它是打包静态库所需的。"
}

# ---------------------------------------------------------------- 5. GNU 备选链路
if (-not $SkipGnuFallback) {
    Write-Step '准备 GNU 备选链路（w64devkit）'
    $w64 = Join-Path $ToolsDir 'w64devkit'
    $w64Gcc = Join-Path $w64 'bin\x86_64-w64-mingw32-gcc.exe'

    if (-not (Test-Path $w64Gcc)) {
        $url = 'https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe'
        $dl = Join-Path $ToolsDir 'w64devkit.7z.exe'
        Write-Host '    下载 w64devkit（约 64 MB）…'
        Invoke-WebRequest -Uri $url -OutFile $dl -UseBasicParsing -TimeoutSec 1800
        Write-Host '    解压…'
        Start-Process -FilePath $dl -ArgumentList @('-y', "-o$ToolsDir") -Wait -NoNewWindow
        Remove-Item $dl -Force
    }
    if (-not (Test-Path $w64Gcc)) { throw "w64devkit 安装失败：未找到 $w64Gcc" }
    Write-Ok 'w64devkit 就绪'

    # GCC >= 13 不再附带 libgcc_eh.a，而 rustc 的 windows-gnu spec 仍硬编码
    # -lgcc_eh。生成一个空归档满足链接器（MinGW 的 SEH 展开路径不需要它）。
    $gccEh = Join-Path $w64 'lib\libgcc_eh.a'
    if (-not (Test-Path $gccEh)) {
        $ar = Join-Path $w64 'bin\x86_64-w64-mingw32-ar.exe'
        $p = Start-Process -FilePath $ar -ArgumentList @('rcs', $gccEh) -Wait -PassThru -NoNewWindow
        if ($p.ExitCode -ne 0) { throw '生成 libgcc_eh.a 失败。' }
        Write-Ok '已生成空壳 libgcc_eh.a'
    } else {
        Write-Ok 'libgcc_eh.a 已存在'
    }

    $gnuToolchain = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu'
    if (-not (Test-Path $gnuToolchain)) {
        Write-Host '    安装 stable-x86_64-pc-windows-gnu …'
        rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
    }
    Write-Ok 'GNU 备选链路就绪（默认不启用）'
}

# ---------------------------------------------------------------- 完成
$sdkMb = [math]::Round(
    (Get-ChildItem $SdkDir -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object Length -Sum).Sum / 1MB, 0)

Write-Host ''
Write-Host "环境就绪（MSVC SDK 约 ${sdkMb} MB）。" -ForegroundColor Green
Write-Host ''
Write-Host '接下来：'
Write-Host '    cargo build --release'
Write-Host '    cargo test'
Write-Host ''
Write-Host '.cargo/config.toml 已指向上述工具，直接 cargo build 即可。'
