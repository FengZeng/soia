/** Simplified Chinese messages. Add new translations to this locale module. */
export const locale = "zh-CN" as const;

import { LANGUAGE_SIMPLIFIED_CHINESE_OPTION } from "../../mock/settings";

export const messages: Record<string, string> = {
    Settings: "设置",
    Reset: "重置",
    Appearance: "外观",
    Language: "语言",
    English: "English",
    [LANGUAGE_SIMPLIFIED_CHINESE_OPTION]: "简体中文",
    Theme: "主题",
    "System (Auto)": "跟随系统",
    Light: "浅色",
    Dark: "深色",
    Graphite: "石墨",
    "Compact Mode": "紧凑模式",
    On: "开",
    Off: "关",
    Show: "显示",
    Editable: "可编辑",
    Hidden: "隐藏",
    Enable: "启用",
    Disable: "禁用",
    Error: "错误",
    Warn: "警告",
    Info: "信息",
    Debug: "调试",
    Trace: "跟踪",
    Automatic: "自动",
    Playback: "播放",
    "Playback Title": "播放标题",
    "Auto-Play Next": "自动播放下一个",
    "Disable Subtitles": "禁用字幕",
    "Default Speed": "默认速度",
    "mpv Config File": "mpv 配置文件",
    "Optional mpv.conf file": "可选的 mpv.conf 文件",
    "Select mpv.conf": "选择 mpv.conf",
    "Seek Step": "跳转步长",
    "Skip Intro For New Videos": "新视频跳过片头",
    "Image Display Duration": "图片显示时长",
    Network: "网络",
    "Always Start at Root": "始终从根目录开始",
    "Multi-thread Download": "多线程下载",
    "Online Subtitles": "在线字幕",
    "Subtitle Languages": "字幕语言",
    "OpenSubtitles API Key (Optional)": "OpenSubtitles API 密钥（可选）",
    "SubSource API Key": "SubSource API 密钥",
    "Leave empty to use Soia's shared API key":
        "留空以使用 Soia 的共享 API 密钥",
    "Create a free key from your SubSource profile":
        "请从 SubSource 个人资料创建免费密钥",
    "Select the yt-dlp executable...": "选择 yt-dlp 可执行文件...",
    "Log path unavailable": "日志路径不可用",
    "Select yt-dlp executable": "选择 yt-dlp 可执行文件",
    "Open log file folder": "打开日志文件夹",
    "Select file": "选择文件",
    Tools: "工具",
    "Video Download Tool (yt-dlp)": "视频下载工具（yt-dlp）",
    "Browser Cookies": "浏览器 Cookies",
    "Max Stream Resolution": "最高流媒体分辨率",
    "Proxy Type": "代理类型",
    "Proxy Server": "代理服务器",
    "Log Level": "日志级别",
    "Log File": "日志文件",
    Experiments: "实验功能",
    "Wallpaper Mode": "壁纸模式",
    "mpv config change will take effect after restart. Restart now?":
        "mpv 配置更改将在重启后生效。现在重启吗？",
    Audio: "音频",
    Output: "输出设备",
    Passthrough: "直通",
    Rendering: "渲染",
    "Custom Shader": "自定义着色器",
    "Select one or more .glsl shader files.": "选择一个或多个 .glsl 着色器文件。",
    "Add Shaders": "添加着色器",
    Clear: "清除",
    "No shader files selected.": "未选择着色器文件。",
    "Use Multiple Shader": "使用多个着色器",
    "General Mode": "通用模式",
    "Anime Mode": "动漫模式",
    About: "关于",
    GitHub: "GitHub",
    Reddit: "Reddit",
    Runtime: "运行环境",
    "Clear All Local Data": "清除所有本地数据",
    "No settings yet": "暂无设置",
    "Add configuration options to start customizing playback.":
        "添加配置选项以开始自定义播放。",
    "Remote Controller": "远程控制",
    "Control playback from a web browser on the same local network":
        "通过同一局域网中的浏览器控制播放",
    "Enable Remote Controller": "启用远程控制",
    "Available on local network": "已在局域网中可用",
    "Remote access is off": "远程访问已关闭",
    paired: "个已配对设备",
    "Disconnect all": "断开全部设备",
    "Preparing…": "准备中…",
    "Show QR Code": "显示二维码",
    "Online subtitle providers": "在线字幕提供商",
    "Clear Cache": "清除缓存",
    "Clearing...": "清除中...",
    Retry: "重试",
    "Open Folder": "打开文件夹",
    Browse: "浏览",
    Unavailable: "不可用",
    Missing: "缺失",
    "Select shader": "选择着色器",
    "Anime Mode: Auto-detect anime videos and apply shaders only for anime.":
        "动漫模式：自动检测动漫视频，仅对动漫应用着色器。",
    "General Mode: Selected shaders will be applied to all videos.":
        "通用模式：所选着色器将应用于所有视频。",
    "Passthrough active": "直通已启用",
    "Selected output unavailable": "所选输出设备不可用",
    "Passthrough unavailable": "直通不可用",
    "Audio output unavailable": "音频输出不可用",
    "The selected yt-dlp file does not exist or is unavailable.":
        "所选 yt-dlp 文件不存在或不可用。",
    "Enter a valid proxy server, for example 127.0.0.1:7890.":
        "请输入有效的代理服务器，例如 127.0.0.1:7890。",
    "Updating...": "更新中...",
    Update: "更新",
    "Installing...": "安装中...",
    "Checking for update... 0%": "正在检查更新... 0%",
    "Installing update...": "正在安装更新...",
    "New version is available!": "有新版本可用！",
    "Success": "成功",
    "Failed": "失败",
    "Set as default": "设为默认应用",
    "Resetting...": "重置中...",
    "Remove shader": "移除着色器",
    "Close QR code": "关闭二维码",
    "Scan to connect": "扫描以连接",
    "Use a phone on the same local network.": "使用同一局域网中的手机。",
    "seconds remaining": "秒后过期",
    "No downloaded subtitles to clear.": "没有可清除的已下载字幕。",
    "Unable to read current default app associations.":
        "无法读取当前默认应用关联。",
    "Soia is now the default app for checked media extensions.":
        "Soia 已成为所选媒体扩展的默认应用。",
    "Some extensions could not be updated automatically.":
        "部分扩展无法自动更新。",
    "Default app was not updated for all checked extensions.":
        "并非所有所选扩展都已更新默认应用。",
    "Failed to set Soia as the default media app.":
        "设置 Soia 为默认媒体应用失败。",
    "Default media app detection is only available on macOS.":
        "默认媒体应用检测仅适用于 macOS。",
    "Unable to detect current default app associations.":
        "无法检测当前默认应用关联。",
    "Default media app detection is unavailable on this system.":
        "此系统无法进行默认媒体应用检测。",
    "Soia is already the default app for checked media extensions.":
        "Soia 已是所选媒体扩展的默认应用。",
    "Some media extensions are not currently handled by Soia.":
        "部分媒体扩展当前未由 Soia 处理。",
    "Factory reset will erase local history, playlists, settings, and network records. UUID will be kept. Continue?":
        "恢复出厂设置将删除本地历史记录、播放列表、设置和网络记录，但会保留 UUID。是否继续？",
    "Factory Reset": "恢复出厂设置",
    "Reset and Restart": "重置并重启",
    Cancel: "取消",
    "Wallpaper Mode change will take effect after restart. Restart now?":
        "壁纸模式更改将在重启后生效。现在重启吗？",
    "Restart Required": "需要重启",
    "Restart now": "立即重启",
    Later: "稍后",
    "Update installed. Restart now to apply the new version?":
        "更新已安装。现在重启以应用新版本吗？",
    Home: "主页",
    History: "历史记录",
    "Choose Files": "选择文件",
    "Tap to choose videos to play": "点击选择要播放的视频",
    "Drag & drop videos to play, or click to browse files":
        "拖放视频即可播放，或点击浏览本地文件",
    "Select one or multiple videos from your device.":
        "可以选择单个或多个视频文件。",
    "Selecting multiple files creates a Playlist automatically.":
        "选择多个文件会自动创建播放列表。",
    "Use the Playlist button in the top-left corner to view or edit it.":
        "使用左上角的播放列表按钮查看或编辑。",
    "Move your cursor to the right side of the window and click to view or edit it.":
        "将光标移到窗口右侧并点击即可查看或编辑播放列表。",
    "No recent plays": "暂无播放记录",
    "Open a file to start building your playback history.":
        "打开文件以开始记录播放历史。",
    "Collapse": "收起",
    Expand: "展开",
    Resume: "继续播放",
    "Unpin history item": "取消固定历史记录",
    "Pin history item": "固定历史记录",
    "Unpin from top": "取消置顶",
    "Pin to top": "置顶",
    "Remove history item": "移除历史记录",
    "Network connections": "网络连接",
    "Not configured": "未配置",
    User: "用户",
    Anonymous: "匿名",
    "Edit connection": "编辑连接",
    "Delete connection": "删除连接",
    "No connections": "暂无连接",
    "Click New to add your first WebDAV connection.":
        "点击“新建”添加第一个 WebDAV 连接。",
    "Parent folder": "上级文件夹",
    Folder: "文件夹",
    "Empty folder": "文件夹为空",
    "No files found for this location.": "此位置未找到文件。",
    Loading: "加载中",
    "Go to parent folder": "返回上级文件夹",
    "Show hidden path folders": "显示隐藏的路径文件夹",
    "Refresh connections": "刷新连接",
    Refreshing: "刷新中",
    Refresh: "刷新",
    "Add connection": "添加连接",
    New: "新建",
    "Sort network entries": "排序网络条目",
    "Alphabetical (A–Z)": "按字母排序（A–Z）",
    "Alphabetical (Z–A)": "按字母排序（Z–A）",
    "Date added (newest)": "按添加日期（最新）",
    "Date added (oldest)": "按添加日期（最早）",
    "Opening connection...": "正在打开连接...",
    "Delete Connection": "删除连接",
    Edit: "编辑",
    Add: "添加",
    Connection: "连接",
    connections: "个连接",
    Delete: "删除",
    "Deleting...": "删除中...",
    Deleting: "删除中",
    "this connection": "此连接",
    Protocol: "协议",
    Name: "名称",
    "Optional (auto fill)": "可选（自动填充）",
    Host: "主机",
    "Share (optional)": "共享（可选）",
    "Leave empty to browse shares": "留空以浏览共享目录",
    Port: "端口",
    Username: "用户名",
    Password: "密码",
    Group: "组",
    Optional: "可选",
    "Default Path": "默认路径",
    "Content Path": "内容路径",
    "Device URL": "设备 URL",
    "Server URL": "服务器 URL",
    Saving: "保存中...",
    "Saving...": "保存中...",
    Save: "保存",
    Create: "创建",
    Connected: "已连接",
    "SMB host is required": "必须填写 SMB 主机",
    "FTP host is required": "必须填写 FTP 主机",
    "DLNA device URL is required": "必须填写 DLNA 设备 URL",
    "Server URL is required": "必须填写服务器 URL",
    "Connection draft is not ready": "连接草稿尚未准备好",
    "Failed to save connection": "保存连接失败",
    "Failed to create connection": "创建连接失败",
    "Failed to delete connection": "删除连接失败",
};

export const translateDynamic = (text: string): string | undefined => {
    const fileNotFound = text.match(/^File not found: (.+)$/);
    if (fileNotFound) return `文件未找到：${fileNotFound[1]}`;

    const shaderOrder = text.match(/^Shader order (\d+)$/);
    if (shaderOrder) return `着色器顺序 ${shaderOrder[1]}`;

    const collapse = text.match(/^Collapse \((\d+)\)$/);
    if (collapse) return `收起（${collapse[1]}）`;

    const showAll = text.match(/^Show all \((\d+)\)$/);
    if (showAll) return `显示全部（${showAll[1]}）`;

    const passthroughCodec = text.match(/^([A-Za-z0-9_-]+) passthrough active$/);
    if (passthroughCodec) return `${passthroughCodec[1].toUpperCase()} 直通已启用`;

    const cleared = text.match(/^Cleared (\d+) file(?:s)? \((.+)\)\.$/);
    if (cleared) return `已清除 ${cleared[1]} 个文件（${cleared[2]}）。`;

    const versionAvailable = text.match(/^Version (.+) is available!$/);
    if (versionAvailable) return `版本 ${versionAvailable[1]} 可用！`;

    const notDefaultForMore = text.match(/^Not default for: (.+), \+(\d+) more\.$/);
    if (notDefaultForMore) {
        return `尚未关联：${notDefaultForMore[1]}，另外 ${notDefaultForMore[2]} 个。`;
    }

    const notDefaultFor = text.match(/^Not default for: (.+)\.$/);
    if (notDefaultFor) return `尚未关联：${notDefaultFor[1]}。`;

    const updateProgress = text.match(/^Downloading update\.\.\. ?(\d+)?%?(.*)$/);
    if (updateProgress) {
        const percent = updateProgress[1] ? ` ${updateProgress[1]}%` : "";
        return `正在下载更新...${percent}${updateProgress[2]}`;
    }

    return undefined;
};
