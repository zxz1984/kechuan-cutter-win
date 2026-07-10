// AI 配置 + 切段需求（动态键值对）+ 多模板
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 单个需求字段（key + value）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqField {
    pub id: String,    // 客户端生成，编辑时稳定
    pub key: String,   // "素材描述" / "用途" / "想要的镜头" / 自定义
    pub value: String, // 字段内容
}

impl ReqField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: uuid_simple(),
            key: key.into(),
            value: value.into(),
        }
    }
}

/// 切段需求（包含一组动态字段 + 时长参数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutRequirement {
    /// 动态字段（key/value 列表，用户可增删改）
    #[serde(default = "default_requirement_fields")]
    pub fields: Vec<ReqField>,
    /// 目标时长（秒）
    #[serde(default = "default_target_duration", alias = "targetDuration")]
    pub target_duration: f64,
    /// 允许偏差（秒）
    #[serde(default = "default_tolerance", alias = "tolerance")]
    pub tolerance: f64,
}

fn default_requirement_fields() -> Vec<ReqField> {
    vec![
        ReqField::new("素材描述", "随手拍/混剪通用素材"),
        ReqField::new("用途", "用于二次剪辑的插入素材"),
        ReqField::new("想要的镜头", "主体清晰、构图好、有明确动作"),
        ReqField::new("不要的镜头", "黑屏/模糊/对焦失败/剧烈抖动/静止"),
    ]
}

fn default_target_duration() -> f64 { 5.0 }
fn default_tolerance() -> f64 { 1.0 }
fn default_use_ai() -> bool { true }
fn default_group_by_video() -> bool { false }
fn default_trim_last_short() -> bool { true }
fn default_fps() -> f64 { 1.0 }
fn default_frame_size() -> u32 { 480 }
fn default_chunk_secs() -> f64 { 120.0 }

impl Default for CutRequirement {
    fn default() -> Self {
        Self {
            fields: default_requirement_fields(),
            target_duration: default_target_duration(),
            tolerance: default_tolerance(),
        }
    }
}

/// 切段模板（命名 + 完整需求）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutTemplate {
    pub id: String,        // uuid
    pub name: String,      // 用户命的名
    pub requirement: CutRequirement,
    #[serde(default)]
    pub builtin: bool,     // 内置不可删
    #[serde(default, alias = "updatedAt")]
    pub updated_at: String,
}

impl CutTemplate {
    pub fn new_builtin(name: String, req: CutRequirement) -> Self {
        Self {
            id: "builtin-default".to_string(),
            name,
            requirement: req,
            builtin: true,
            updated_at: "".to_string(),
        }
    }
}

/// AI + 切段总配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIConfig {
    // 同时认 snake_case（旧配置文件格式）和 camelCase（Rust rename_all 输出的格式）
    #[serde(alias = "base_url", default)]
    pub base_url: String,
    #[serde(alias = "api_key", default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_fps")]
    pub fps: f64,
    #[serde(default = "default_frame_size", alias = "frame_max_size")]
    pub frame_max_size: u32,
    /// AI 分析每片窗口时长（秒），0=自动 120
    #[serde(default = "default_chunk_secs")]
    pub chunk_secs: f64,
    #[serde(default = "default_use_ai", alias = "use_ai")]
    pub use_ai: bool,
    #[serde(default = "default_group_by_video", alias = "group_by_video")]
    pub group_by_video: bool,
    /// 纯裁切：最后一段时长不足目标时长就不保存
    #[serde(default = "default_trim_last_short", alias = "trim_last_short_segment")]
    pub trim_last_short_segment: bool,
    /// 当前切段需求（直接编辑用）
    #[serde(default)]
    pub requirement: CutRequirement,
    /// 当前载入的模板 id（None 表示"未保存的自由编辑"）
    #[serde(default, alias = "current_template_id")]
    pub current_template_id: Option<String>,
    /// 上次选择的输入文件夹（重启后自动填回）
    #[serde(default, alias = "last_input_folder")]
    pub last_input_folder: String,
    /// 上次选择的输出文件夹（重启后自动填回）
    #[serde(default, alias = "last_output_folder")]
    pub last_output_folder: String,
    /// 用过的素材处理方式：none / delete / move
    #[serde(default = "default_used_video_action", alias = "used_video_action")]
    pub used_video_action: String,
}

fn default_used_video_action() -> String { "none".to_string() }

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            // 默认值全部留空，避免把开发用的 API key / 中转站地址硬编码进 binary
            // 用户首次打开应用需要在「AI 设置」里填入自己的 base_url / api_key / model
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            fps: default_fps(),
            frame_max_size: default_frame_size(),
            chunk_secs: default_chunk_secs(),
            use_ai: default_use_ai(),
            group_by_video: default_group_by_video(),
            trim_last_short_segment: default_trim_last_short(),
            requirement: CutRequirement::default(),
            current_template_id: None,
            last_input_folder: String::new(),
            last_output_folder: String::new(),
            used_video_action: default_used_video_action(),
        }
    }
}

// =================== 文件读写 ===================

fn config_dir() -> Result<PathBuf, String> {
    // macOS 真实 HOME 检测：$HOME 可能被 Claude Code sandbox 等环境改成沙盒路径
    // 用 `/usr/bin/id -un` 拿真正的 OS 用户名（不读 $HOME 环境变量），拼真实路径
    let home = get_real_home().ok_or_else(|| "无法获取用户主目录".to_string())?;
    let dir = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("com.cola.cutter");
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    Ok(dir)
}

/// 获取真实用户主目录，绕过被 sandbox 改过的 $HOME
fn get_real_home() -> Option<String> {
    use std::process::Command;

    // 方法 1：$HOME 如果是 /Users/<xxx>/... 形式就直接用
    if let Ok(home) = std::env::var("HOME") {
        if home.starts_with("/Users/") {
            return Some(home);
        }
    }

    // 方法 2：用 whoami / id 拿当前用户名，拼 /Users/<username>
    let username = Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())?;

    if username.is_empty() { return None; }
    Some(format!("/Users/{}", username))
}

fn config_path() -> Result<PathBuf, String> { Ok(config_dir()?.join("ai_config.json")) }
fn templates_path() -> Result<PathBuf, String> { Ok(config_dir()?.join("templates.json")) }

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    // 时间戳 + 原子计数器，保证快速连续调用时也唯一
    format!("u{}_{}", n, c)
}

// =================== AI 配置 ===================

#[tauri::command]
pub fn load_ai_config() -> Result<AIConfig, String> {
    let path = config_path()?;
    use std::io::Write;
    let log_path = std::env::var("HOME").unwrap_or_default() + "/cola-cutter-debug.log";
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(f, "[load_ai_config] path={:?} exists={}", path, path.exists());
    }
    if !path.exists() {
        return Ok(AIConfig::default());
    }
    let s = fs::read_to_string(&path).map_err(|e| format!("读配置失败: {}", e))?;
    let cfg: AIConfig = serde_json::from_str(&s).map_err(|e| format!("解析配置失败: {}", e))?;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(f, "[load_ai_config] OK use_ai={}, model={}, base_url={:?}, last_input_folder={:?}, last_output_folder={:?}",
            cfg.use_ai, cfg.model, cfg.base_url, cfg.last_input_folder, cfg.last_output_folder);
    }
    Ok(cfg)
}

#[tauri::command]
pub fn save_ai_config(config: AIConfig) -> Result<(), String> {
    // 写 log
    use std::io::Write;
    let log_path = std::env::var("HOME").unwrap_or_default() + "/cola-cutter-debug.log";
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(f, "[save_ai_config] use_ai={}, fps={}, frame_max_size={}, chunk_secs={}, requirement.target_duration={}",
            config.use_ai, config.fps, config.frame_max_size, config.chunk_secs, config.requirement.target_duration);
    }
    let path = config_path()?;
    let s = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("序列化配置失败: {}", e))?;
    fs::write(&path, s).map_err(|e| format!("写配置失败: {}", e))?;
    Ok(())
}

// =================== 模板 CRUD ===================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateList {
    pub templates: Vec<CutTemplate>,
}

fn load_templates_internal() -> Result<TemplateList, String> {
    let path = templates_path()?;
    if !path.exists() {
        // 第一次：写一个内置 demo
        let builtin = CutTemplate::new_builtin(
            "通用素材（内置）".to_string(),
            CutRequirement::default(),
        );
        let list = TemplateList { templates: vec![builtin] };
        let s = serde_json::to_string_pretty(&list)
            .map_err(|e| format!("序列化内置模板失败: {}", e))?;
        fs::write(&path, s).map_err(|e| format!("写内置模板失败: {}", e))?;
        return Ok(list);
    }
    let s = fs::read_to_string(&path).map_err(|e| format!("读模板失败: {}", e))?;
    let list: TemplateList = serde_json::from_str(&s)
        .map_err(|e| format!("解析模板失败: {}", e))?;
    Ok(list)
}

fn save_templates_internal(list: &TemplateList) -> Result<(), String> {
    let path = templates_path()?;
    let s = serde_json::to_string_pretty(list)
        .map_err(|e| format!("序列化模板失败: {}", e))?;
    fs::write(&path, s).map_err(|e| format!("写模板失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn list_templates() -> Result<TemplateList, String> {
    load_templates_internal()
}

#[tauri::command]
pub fn save_template(name: String, requirement: CutRequirement, existing_id: Option<String>) -> Result<CutTemplate, String> {
    let mut list = load_templates_internal()?;

    let template = if let Some(id) = existing_id {
        // 覆盖已存在
        let pos = list.templates.iter().position(|t| t.id == id).ok_or_else(|| format!("模板不存在: {}", id))?;
        let t = CutTemplate {
            id: id.clone(),
            name,
            requirement,
            builtin: list.templates[pos].builtin,
            updated_at: now_iso(),
        };
        list.templates[pos] = t.clone();
        t
    } else {
        // 新建
        let t = CutTemplate {
            id: uuid_simple(),
            name,
            requirement,
            builtin: false,
            updated_at: now_iso(),
        };
        list.templates.push(t.clone());
        t
    };

    save_templates_internal(&list)?;
    Ok(template)
}

#[tauri::command]
pub fn delete_template(id: String) -> Result<(), String> {
    let mut list = load_templates_internal()?;
    let pos = list.templates.iter().position(|t| t.id == id).ok_or_else(|| format!("模板不存在: {}", id))?;
    if list.templates[pos].builtin {
        return Err("内置模板不可删除".to_string());
    }
    list.templates.remove(pos);
    save_templates_internal(&list)?;
    Ok(())
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("ts:{}", secs)
}
