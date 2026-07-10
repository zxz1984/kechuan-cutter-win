# ✂ cola-cutter —— 可乐裁切

> 长视频自动切段工具，让手动剪辑的时间从几小时缩短到几分钟。

**当前版本**：v0.5
**打包方式**：Tauri 2.1 (.dmg onedir 模式)
**主语言**：Rust + TypeScript/React

---

## 🎯 功能清单

### 核心功能

| 功能 | 说明 | 入口文件 |
|------|------|---------|
| ✂ **纯裁切模式** | 按固定时长等分切段，不调用 AI，速度极快 | `src-tauri/src/cutter.rs` → `run_pure_cut` |
| 🤖 **AI 裁切模式** | 抽帧 → 视觉模型分析 → 智能挑选优质片段 | `src-tauri/src/cutter.rs` → `run_ai_cut` |
| 📁 **扫描文件夹** | 自动识别 mp4/mov/mkv/avi/webm/flv/m4v | `src-tauri/src/cutter.rs` → `scan_folder` |
| 💾 **配置管理** | AI 接口 / 模板 / 切段参数全部可保存 | `src-tauri/src/config.rs` |
| 📑 **模板管理** | 保存/载入/删除切段需求模板 | `src-tauri/src/config.rs` → `save_template` |
| 🔌 **测试连接** | 测试 AI 接口连通性 | `src-tauri/src/vision.rs` → `test_connection` |

### 输出组织

- **平铺输出**：`<视频名>_seg_NNN.mp4` 全部放输出目录
- **按视频分组**：每个视频一个子目录 `<视频名>/seg_NNN.mp4`

---

## 📁 项目结构

```
cola-cutter/
├── src/                          # React 前端
│   ├── App.tsx                   # ⭐ 主界面（裁切模式选择、参数配置、运行按钮）
│   ├── App.css                   # 样式
│   ├── main.tsx                  # React 入口
│   └── index.css
│
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── main.rs               # Rust 入口
│   │   ├── lib.rs                # ⭐ Tauri 命令注册
│   │   ├── cutter.rs             # ⭐ 裁切业务核心（纯裁切/AI裁切/切段）
│   │   ├── vision.rs             # ⭐ 视觉模型调用、抽帧、水印、解析 JSON
│   │   ├── ffmpeg.rs             # FFmpeg/ffprobe 路径解析与时长探测
│   │   └── config.rs             # AIConfig / 模板存储（JSON 文件）
│   ├── Cargo.toml
│   ├── tauri.conf.json           # ⭐ 窗口配置 / 打包配置
│   ├── icons/                    # 应用图标（.icns）
│   └── capabilities/
│       └── default.json          # ⭐ Tauri 权限清单
│
├── bin/
│   └── ffmpeg-darwin-x64         # 内置 FFmpeg（macOS）
│
├── index.html
├── package.json                  # npm 配置
├── vite.config.ts                # Vite 构建配置
├── tsconfig.json
└── .gitignore
```

---

## 🏗️ 技术架构

### 数据流（AI 模式）

```
用户选文件夹
    ↓
scan_folder (Rust 扫描视频)
    ↓
[每个视频]
    ↓
probe_duration (ffprobe 拿时长)
    ↓
切分时间片 chunk_secs=120s
    ↓
[每个分片]
    ↓
ffmpeg 抽帧 (fps / 缩放至 frameMaxSize)
    ↓
watermark_frames (加水印避免 AI 直接抄原图)
    ↓
base64 编码
    ↓
HTTP POST → OpenAI 兼容接口
    ↓
解析 AI 返回的 JSON 数组 [{start, end, reason}]
    ↓
切段 (cut_shots → cut_one 调用 ffmpeg)
```

### 前后端通信（Tauri 命令）

| Rust 命令 | 前端调用 | 用途 |
|----------|---------|------|
| `scan_folder` | `invoke('scan_folder', { folder })` | 扫描文件夹 |
| `get_video_duration` | `invoke('get_video_duration', { path })` | 单个视频时长 |
| `run_pure_cut` | `invoke('run_pure_cut', { inputFolder, outputFolder, targetDuration, groupByVideo })` | **纯裁切**（独立命令，绝不走 AI） |
| `run_ai_cut` | `invoke('run_ai_cut', { inputFolder, outputFolder, config })` | **AI 裁切**（独立命令） |
| `run_batch_cut` | `invoke('run_batch_cut', { ... })` | 旧版总入口（保留兼容） |
| `load_ai_config` | `invoke('load_ai_config')` | 启动时加载 AI 配置 |
| `save_ai_config` | `invoke('save_ai_config', { config })` | 保存 AI 配置 |
| `list_templates` | `invoke('list_templates')` | 列出切段模板 |
| `save_template` | `invoke('save_template', { name, requirement, existingId })` | 保存/覆盖模板 |
| `delete_template` | `invoke('delete_template', { id })` | 删除模板 |
| `test_connection` | `invoke('test_connection', { config })` | 测试 AI 连接 |

### 关键事件

| 事件名 | payload | 触发时机 |
|--------|---------|---------|
| `cut-progress` | `{ current, total, item, stage, ... }` | 每个阶段：probe / extract_frames / loading_frames / ai_analyzing / ai_done / cutting / pure_cutting / done |
| `cut-done` | `{ total }` | 全部处理完成 |

---

## ⚙️ 关键配置（AIConfig）

```rust
pub struct AIConfig {
    pub base_url: String,        // OpenAI 兼容接口 base URL
    pub api_key: String,         // API Key
    pub model: String,           // 模型名
    pub fps: f64,                // 抽帧率（帧/秒）
    pub frame_max_size: u32,     // 帧最大边长（360/480/720/1080）
    pub use_ai: bool,            // 是否启用 AI 模式
    pub group_by_video: bool,    // 输出组织方式
    pub requirement: CutRequirement,
    pub current_template_id: Option<String>,
    pub chunk_secs: f64,         // 分片时长（默认 120s）
}
```

---

## 🚀 开发与运行

### 开发模式

```bash
cd cola-cutter
npm install
npm run tauri dev
```

### 打包（macOS）

```bash
npm run tauri build
# 产物在: src-tauri/target/release/bundle/dmg/
```

> ⚠️ **必须使用 onedir 模式**：当前配置已经是 onedir，能避免双图标、加快启动速度。

---

## 🐛 已知问题 & 修复历史

### ✅ 已修复：模式选择混乱（最关键）

**症状**：选择「纯裁切」却跑 AI；选择「AI 裁切」却跑纯裁切。

**根因**：
- `use_ai` 字段在前端 state、配置存储、命令参数多处传递，容易状态污染
- 旧版 `run_batch_cut` 用单一入口根据 `use_ai` 分支，调试困难

**解决方案**：**物理隔离** —— 创建两个独立 Tauri 命令：
- [run_pure_cut](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src-tauri/src/cutter.rs#L120-L154) —— 强制走纯裁剪，签名不含 AIConfig
- [run_ai_cut](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src-tauri/src/cutter.rs#L157-L189) —— 强制走 AI，签名必传 AIConfig

前端按钮直接调用对应函数，绝不经过 `use_ai` 判断。

---

### ✅ 已修复：AI 返回非 JSON 数组

**症状**：AI 响应只有纯文本分析，无 JSON 数组。

**解决方案**：在 `parse_segments_json` 增加回退逻辑，从纯文本中用正则提取时间段。

---

### ✅ 已修复：启动慢 + 双图标

**根因**：单文件模式（onefile）每次启动需解压 + 默认配置错乱。

**解决方案**：
- 打包用 onedir 模式
- 检查 `tauri.conf.json` 的 bundle 资源映射
- 确保 macOS 的 `LSUIElement` 配置正确

---

## 📊 性能参考

| 模式 | 35 个 1 分钟视频 | 主要耗时 |
|------|----------------|---------|
| 纯裁切 | ~3-5 分钟 | FFmpeg 切段 |
| AI 裁切 | ~15-30 分钟 | 抽帧 + AI 分析 |

AI 模式瓶颈是 API 速度，本地 GPU 推理可大幅加速（后续可考虑）。

---

## 🔮 下一步计划

- [ ] 批量导出预设
- [ ] 多线程切段（当前单线程）
- [ ] 智能场景检测（不依赖 AI 模型）
- [ ] 多平台比例切换（9:16 / 16:9 / 1:1）
- [ ] 与 cola-mixer 集成（裁切→混剪流水线）

---

## 📝 给开发智能体的指引

修改本软件前请重点看：
1. [App.tsx](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src/App.tsx) —— 前端主入口（约 890 行）
2. [cutter.rs](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src-tauri/src/cutter.rs) —— 裁切核心（约 540 行）
3. [vision.rs](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src-tauri/src/vision.rs) —— AI 调用与 JSON 解析
4. [lib.rs](file:///Users/zxz/Documents/trae_projects/cola-suite/cola-cutter/src-tauri/src/lib.rs) —— 命令注册

**调试日志**：运行时会写 `~/cola-cutter-debug.log`，遇到问题先看这里。