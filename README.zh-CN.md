<h1 align="center">
  <img src="./src-tauri/icons/icon.png" alt="Soia" width="128" />
  <br>
  Soia
  <br>
</h1>

<p align="center">
🎬 HDR 与杜比视界 · 🌐 WebDAV + DLNA + SMB 流媒体播放 · 📱 浏览器远程控制器
</p>

<p align="center">
<b><a href="https://github.com/FengZeng/soia/releases">⬇️ 下载最新版本</a> · <a href="https://github.com/FengZeng/soia/issues">🐞 报告问题</a></b>
</p>

[English](README.md) · 简体中文

![Soia 应用预览](docs/assets/screenshots/Soia.webp)

**Soia** 是一个基于 mpv、专注于视频渲染与网络播放的跨平台媒体播放器，支持基于浏览器的远程控制和投屏等特色功能。

---

## 为什么选择 Soia？

### 1. 支持 Dolby Vision 播放（macOS 和 Windows 平台）

<sub style="padding-left: 2em;">*Linux 目前不支持 Dolby Vision。*</sub>

### 2. 更强的在线视频支持（YouTube、 哔哩哔哩等）

- 支持将 YouTube 播放列表导入 Soia，作为原生播放列表使用。
- 某些链接在其他基于 mpv 的播放器中无法播放，但在 Soia 中可能可以正常播放。
- 支持并行下载，有助于保持在线视频播放流畅，尤其是在网络不稳定时。

### 3. 播放网络媒体库中的内容

无需先将视频下载到电脑，即可浏览并播放来自 DLNA、SMB/Samba 和 WebDAV 的视频流。

### 4. 投屏当前正在观看的内容

将当前播放的视频——无论是本地文件，还是来自 DLNA、SMB、WebDAV 或 YouTube 的在线视频或网络视频流——投放到 DLNA 接收器或 Chromecast 设备。

> **注意：** 投屏期间请保持 Soia 运行。Soia 会在源和接收器之间中继媒体流。

### 5. 基于浏览器的远程控制器

扫描二维码，即可连接手机或其他浏览器。远程控制器支持：

- 播放、暂停、快进/快退和音量等基本播放控制
- 播放列表浏览与播放
- 网络内容浏览与播放
- 音轨和字幕轨选择

在设置中启用“远程控制器”，然后从设置页面或播放上下文菜单中显示二维码，以完成配对和连接。多个远程设备可以同时控制同一个播放器。

![浏览器远程控制器](docs/assets/screenshots/remote-controller.webp)

#### 工作原理

桌面应用和网页远程控制器是同一个播放后端的两个独立客户端。

![共享后端架构](docs/assets/diagrams/shared-backend.webp)

## 更多播放功能

- macOS 和 Windows 上的画中画（PiP）
- 双字幕，方便双语观看
- 字幕模糊匹配
- 通过 OpenSubtitles 和 SubSource 在线搜索字幕
- 字体、颜色、大小和位置等高级字幕外观控制
- 用于高质量缩放和渲染的自定义着色器
- M3U（IPTV）解析与播放
- 带实时速度指示器的智能缓冲
- 通过历史记录追踪实现断点续播

---

## 安装

请从[发布页面](https://github.com/FengZeng/soia/releases)下载。

在 macOS 上，可以使用 Homebrew 安装：

```bash
brew tap FengZeng/soia
brew install --cask soia
```

在 Windows 上，可以使用 WinGet 安装：

```powershell
winget install soia
```

也可以自行构建。Soia 支持 macOS 13 及更高版本、Windows 和 Linux。
Linux 构建版本仅支持 Wayland，已在 Ubuntu 和 Fedora 的 Wayland 会话中进行测试。

## 常见问题

问：macOS 提示“Soia 已损坏，无法打开”，或无法验证应用不包含恶意软件。

答：这是因为应用尚未使用 Apple Developer ID 证书签名，macOS 可能会在首次启动时阻止打开。

简单解决方法（推荐）：

1. 右键点击 Soia.app
2. 点击“打开”
3. 在对话框中再次点击“打开”

如果仍然无法打开，请运行：

```bash
xattr -r -d com.apple.quarantine /Applications/Soia.app
```

也可以前往“系统设置”→“隐私与安全性”，点击“仍要打开”（该选项会在应用启动被阻止后出现）。

## 技术栈

- 前端：Vue 3 + TypeScript + Vite
- 应用运行时：Tauri v2
- 后端：Rust
- 播放引擎：libmpv
- 数据持久化：SQLite（`media.db`）+ JSON 状态文件

## 快速开始

1. 前置条件

   请确保已安装以下软件：

   - Node.js 18 及更高版本与 pnpm 10.x
   - Rust（稳定版工具链）
   - 适用于具体平台的 Tauri 构建依赖

2. 设置

   ```bash
   # 自动准备运行时库
   pnpm install
   ```

3. 运行

   ```bash
   # 启动并自动注入库路径
   pnpm tauri dev
   ```

## 构建与打包

常用的发布构建命令：

```bash
pnpm bundle:mac:release
pnpm bundle:linux:release
pnpm bundle:win:release
```

## 键盘快捷键

- `Space`：播放/暂停
- `Left / Right`：快退/快进（步长可在设置中调整）
- `I`：切换播放信息面板
- 双击视频区域：切换全屏
- 播放期间单击鼠标中键：隐藏或显示控制栏；隐藏后 3 秒内鼠标移动仍会被抑制

## 数据存储

应用数据存储在 Tauri 的本地应用数据目录中，包括：

- `media.db`：播放列表、播放列表条目、播放历史，以及本地安装/设备元数据
- `state.json`：界面状态和偏好设置
- `network_connections.json`：已保存的网络连接
- `thumbnails/`：为“正在播放”页面捕获的封面图

## 安全提示

当前，已保存的网络凭据以明文形式保存在 `network_connections.json` 中。请避免在共享设备上使用敏感的生产环境凭据。

## 故障排除

- 如果 Linux 构建因找不到 `glib-2.0`、`gdk-3.0` 或 `*.pc` 文件而失败，请安装 Ubuntu 依赖：

```bash
sudo apt update
sudo apt install -y \
    build-essential \
    curl \
    wget \
    file \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    pkg-config \
    libwebkit2gtk-4.1-dev
```

- Linux 运行时提示：当前构建版本仅面向 Ubuntu Wayland 会话；不支持在纯 `X11` 环境下启动。

- 如果构建因找不到 `libmpv` 而失败，请运行：

```bash
pnpm setup:libs
```

- 如果 `pnpm setup:libs` 失败，请确认可以访问以下发布地址：
  - `https://github.com/FengZeng/mpv/releases/tag/v0.41.0-r20`
  - 或将 `MPV_RELEASE_ASSET_URL` 设置为直接资源 URL 后重试。

- 如果 Linux/Windows 打包脚本提示缺少运行时清单，请在目标平台生成：

```bash
pnpm sync:runtime:linux
pnpm sync:runtime:win
```

- 如果你有一个用于开发测试的本地 `mpv + dependencies` 目录，请使用：

```bash
pnpm setup:libs /absolute/path/to/mpv-bundle
```

---

## 许可证

本项目仅以 GNU 通用公共许可证 v3.0（`GPL-3.0-only`）授权。
完整文本请参阅 [`LICENSE`](LICENSE)。
