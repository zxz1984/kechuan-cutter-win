// K2.6 视觉客户端：OpenAI 兼容 + SSE 解析
// AI 直接出"切段方案"（含 start/end/reason），后端按方案切
use crate::config::{AIConfig, CutRequirement};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FrameShot {
    pub path: String,
    pub t: f64,
    pub data_base64: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CutShot {
    pub start: f64,
    pub end: f64,
    pub reason: Option<String>,
}

/// 写日志到固定文件（macOS bundle 没 stdout）
fn log_to_file(msg: &str) {
    use std::io::Write;
    let path = std::env::var("HOME").unwrap_or_default() + "/cola-cutter-debug.log";
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), msg);
    }
}

macro_rules! dlog { ($($arg:tt)*) => { log_to_file(&format!($($arg)*)); } }

/// 测试 AI 连接
#[tauri::command]
pub async fn test_connection(cfg: AIConfig) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client: {}", e))?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let body = json!({
        "model": cfg.model,
        "messages": [{
            "role": "user",
            "content": [{"type": "text", "text": "ping, reply 'pong'"}]
        }],
        "max_tokens": 16,
        "temperature": 0.1,
    });
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    dlog!("[vision-test] HTTP {}", status);
    let text = resp.text().await.map_err(|e| format!("读响应失败: {}", e))?;
    dlog!("[vision-test] 收到 {} 字符, 前 300 字符: {}", text.len(), &text.chars().take(300).collect::<String>());
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, &text[..text.len().min(500)]));
    }
    let content = extract_content_sse_or_json(&text);
    Ok(format!(
        "OK (HTTP {}, 回复: {})",
        status,
        if content.is_empty() { "(空)".to_string() } else { content }
    ))
}

/// 调 AI：直接出切段方案
pub async fn ai_propose_segments(
    cfg: &AIConfig,
    req: &CutRequirement,
    frames: &[FrameShot],
    video_duration: f64,
    chunk_offset: f64,
) -> Result<Vec<CutShot>, String> {
    if frames.is_empty() {
        return Err("无帧可分析".to_string());
    }
    const BATCH: usize = 16;
    let mut all_shots: Vec<CutShot> = Vec::new();

    for (batch_idx, chunk) in frames.chunks(BATCH).enumerate() {
        let prompt = build_prompt(req, chunk, video_duration, chunk_offset);
        let shots = call_vision(cfg, &prompt, chunk)
            .await
            .map_err(|e| format!("第 {} 批分析失败: {}", batch_idx + 1, e))?;
        all_shots.extend(shots);
    }

    // 长度过滤：
    // - min_dur：每段必须 >= target_duration，但 target 太小时（如 1.5s）兜底到 2s（避免切出太碎片段）
    // - max_dur：每段最长 target_duration + tolerance（用户 UI 上设的）
    // - 偏差只能往上加：每段时长必须 >= target_duration（不能比目标短）
    let min_dur = req.target_duration.max(2.0);
    let max_dur = req.target_duration + req.tolerance;
    all_shots.retain(|s| {
        let d = s.end - s.start;
        d >= min_dur && d <= max_dur
    });

    Ok(all_shots)
}

/// 根据切段需求构造 prompt（动态字段 + 时间点摘要 + 图上有水印）
fn build_prompt(req: &CutRequirement, frames: &[FrameShot], video_duration: f64, chunk_offset: f64) -> String {
    let target = req.target_duration;
    let tol = req.tolerance;
    // 允许偏差只能往上加：每段必须 ≥ target，最多 target + tolerance
    let min = target;
    let max = target + tol;

    let times: Vec<String> = frames.iter().map(|f| format!("{:.2}s", f.t)).collect();
    // 窗口偏移说明（让 AI 知道这是视频的哪一段）
    let window_info = if chunk_offset > 0.0 {
        format!("（这是整段视频的 [{}]~[{}]s 切片，绝对时间戳要加 {}{}）",
                chunk_offset, chunk_offset + video_duration, chunk_offset, "s")
    } else {
        String::new()
    };

    let summary = if times.len() <= 15 {
        times.join(", ")
    } else {
        let n = times.len();
        let head: Vec<String> = times.iter().take(5).cloned().collect();
        let tail: Vec<String> = times.iter().rev().take(5).rev().cloned().collect();
        let mid_idx = n / 2;
        let mid: Vec<String> = times.iter().skip(mid_idx.saturating_sub(2)).take(5).cloned().collect();
        format!("前段: {} → 中段: {} → 后段: {} (共 {} 帧)",
                head.join(", "), mid.join(", "), tail.join(", "), n)
    };

    let mut context = String::new();
    for f in &req.fields {
        let k = f.key.trim();
        let v = f.value.trim();
        if k.is_empty() || v.is_empty() { continue; }
        context.push_str(&format!("\n**{}**：{}", k, v));
    }

    format!(
        "你是一个专业的视频素材挑选专家。{context}\n\n         这是一段{:.0}秒视频的连续抽帧，共 {} 帧。每张图左上角**已画上时间戳水印**（如 t: 12.5s），\n         时间分布：{summary}\n\n         **任务**：从这些帧里挑出最值得保留的\"插入素材镜头\"{window_info}，每段 {target:.1} 秒（允许 {min:.1}~{max:.1} 秒）。\n\n         **跳过**：黑屏/纯色/过曝/对焦失败/剧烈抖动/长时间静止。\n\n         **输出严格 JSON 数组**（不要任何其他文字）：\n         ```\n         [\n         {{ \"start\": <秒>, \"end\": <秒>, \"reason\": \"<一句话为什么选它>\" }},\n         ...\n         ]\n```\n\n         要求：\n         - **以图上时间戳为准**（图上水印的 t 才是真实时间）\n         - start/end 用时间戳\n         - 只选真正可用的好镜头，宁缺毋滥\n         - 每段尽量在 {min:.1}~{max:.1} 秒之间\n         - reason 中文一句话精炼",
        video_duration,
        frames.len(),
        summary = summary,
        context = context,
        target = target,
        min = min,
        max = max,
        window_info = window_info,
    )
}

async fn call_vision(
    cfg: &AIConfig,
    prompt_text: &str,
    frames: &[FrameShot],
) -> Result<Vec<CutShot>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| format!("client: {}", e))?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

    let mut content: Vec<Value> = Vec::new();
    for f in frames {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{}", f.data_base64) }
        }));
    }
    content.push(json!({ "type": "text", "text": prompt_text }));

    let body = json!({
        "model": cfg.model,
        "stream": false,           // 强制非流式，一次性返回
        "messages": [{ "role": "user", "content": content }],
        "max_tokens": 2000,
        "temperature": 0.2,
    });

    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    dlog!("[vision-cut] HTTP {}", status);
    let text = resp.text().await.map_err(|e| format!("读响应失败: {}", e))?;
    dlog!("[vision-cut] 收到响应 {} 字符, 前 500 字符: {}", text.len(), &text.chars().take(500).collect::<String>());

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, &text[..text.len().min(800)]));
    }

    let content_str = extract_content_sse_or_json(&text);
    dlog!("[vision-cut] 解析 content_str 长 {} 字符, 前 300: {}", content_str.len(), &content_str.chars().take(300).collect::<String>());
    let shots = parse_segments_json(&content_str)?;
    dlog!("[vision-cut] 解析得到 {} 个 shot", shots.len());
    Ok(shots)
}

fn extract_content_sse_or_json(raw: &str) -> String {
    // 1) SSE 流式响应（vllm 等中转站默认返回 data: {...}\n\n 格式）
    if raw.contains("data:") {
        let mut combined = String::new();
        for line in raw.lines() {
            let line = line.trim();
            // 跳过空行、SSE 注释行、SSE 结束标记
            if line.is_empty() || line.starts_with(":") || line == "data: [DONE]" {
                continue;
            }
            // 去掉 "data: " 前缀，取出 JSON 片段
            if let Some(payload) = line.strip_prefix("data:") {
                let payload = payload.trim();
                if payload.is_empty() { continue; }
                if let Ok(v) = serde_json::from_str::<Value>(payload) {
                    // 优先取 message.content（非流式单块场景）
                    if let Some(s) = v["choices"][0]["message"]["content"].as_str() {
                        combined.push_str(s);
                        continue;
                    }
                    // 流式 delta.content
                    if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
                        combined.push_str(s);
                    }
                }
            }
        }
        if !combined.is_empty() {
            return combined;
        }
    }

    // 2) 标准 JSON（非流式）
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        if let Some(s) = v["choices"][0]["message"]["content"].as_str() {
            return s.to_string();
        }
    }

    // 3) 兜底：原样返回
    raw.to_string()
}

fn parse_segments_json(content: &str) -> Result<Vec<CutShot>, String> {
    let s = content.trim();
    let s = if s.starts_with("```") {
        let s = s.trim_start_matches("```json").trim_start_matches("```");
        s.trim_end_matches("```").trim()
    } else {
        s
    };

    // 尝试找到 JSON 数组
    if let Some(start) = s.find('[') {
        if let Some(end) = s.rfind(']') {
            let sub = &s[start..=end];
            if let Ok(arr) = serde_json::from_str::<Value>(sub) {
                if let Some(items) = arr.as_array() {
                    let mut out = Vec::new();
                    for item in items {
                        fn parse_ts(v: &Value) -> f64 {
                            if let Some(n) = v.as_f64() { return n; }
                            if let Some(s) = v.as_str() {
                                return s.trim_end_matches('s').trim().parse().unwrap_or(0.0);
                            }
                            0.0
                        }
                        let start = parse_ts(&item["start"]);
                        let end = parse_ts(&item["end"]);
                        if end <= start { continue; }
                        let reason = item.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                        out.push(CutShot { start, end, reason });
                    }
                    if !out.is_empty() {
                        return Ok(out);
                    }
                }
            }
        }
    }

    // 回退：从纯文本分析中提取时间段
    // AI 分析格式通常是："- 0.00s: Shows a metro sign..." 或 "0.00s: ..."
    dlog!("[vision-cut] JSON 解析失败，尝试从文本提取时间段");
    let fallback = parse_text_analysis(s);
    if fallback.is_empty() {
        return Err(format!("无法从 AI 响应中提取切段信息: {}", &s[..s.len().min(200)]));
    }
    Ok(fallback)
}

/// 从 AI 文本分析中回退提取时间段
fn parse_text_analysis(text: &str) -> Vec<CutShot> {
    let mut out: Vec<CutShot> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    // 负面关键词：包含这些的时间段应跳过
    let negative_keywords = ["黑屏", "纯色", "过曝", "对焦失败", "剧烈抖动", "静止", "模糊", "motion blur"];
    // 正面关键词
    let _positive_keywords = ["清晰", "构图", "主体", "动作", "stable", "clear"];

    let mut current_start: Option<f64> = None;
    let mut current_reason: Option<String> = None;

    for line in lines {
        let line = line.trim();
        // 匹配时间戳格式：0.00s 或 t: 12.5s
        let re_time = regex::Regex::new(r"(?i)(?:t:\s*)?(\d+\.?\d*)\s*s").ok();
        let time_opt = re_time.and_then(|re| {
            re.captures(line).and_then(|cap| cap.get(1))
                .and_then(|m| m.as_str().parse::<f64>().ok())
        });

        if let Some(t) = time_opt {
            let is_negative = negative_keywords.iter().any(|kw| line.contains(kw));
            let is_positive = !is_negative && line.len() > 10;

            if is_positive {
                if current_start.is_none() {
                    current_start = Some(t);
                    current_reason = Some(line.to_string());
                }
            } else {
                // 遇到负面描述，结束当前片段
                if let (Some(start), Some(_)) = (current_start, &current_reason) {
                    if t > start && t - start >= 1.0 {
                        out.push(CutShot {
                            start,
                            end: t,
                            reason: current_reason.clone(),
                        });
                    }
                }
                current_start = None;
                current_reason = None;
            }
        }
    }

    // 处理最后一段
    if let (Some(start), Some(_)) = (current_start, &current_reason) {
        let end = start + 5.0;
        out.push(CutShot { start, end, reason: current_reason });
    }

    // 合并相邻片段和去重
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn read_file_base64(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读文件失败 {}: {}", path, e))?;
    if bytes.len() > 1_500_000 {
        return Err(format!("文件过大 (>1.5MB): {}", path));
    }
    Ok(B64.encode(&bytes))
}
use std::process::Command;

/// 跨平台找系统字体（macOS / Windows / Linux）
/// 用于给图片加水印时指定 -font 参数。返回 None 表示没找到。
pub fn resolve_system_font() -> Option<String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\Arial.ttf",
            "C:\\Windows\\Fonts\\Arial.ttf",  // 大小写都试
        ]
    } else {
        // Linux
        &[
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ]
    };
    for p in candidates {
        if std::path::Path::new(p).exists() {
            return Some((*p).to_string());
        }
    }
    None
}

/// 给每张抽帧图加时间水印（t: 12.5s）
/// `max_size` 是图片最大边（px），用于自适应水印字号
pub fn watermark_frames(frames: &[(f64, String)], max_size: u32) -> Result<(), String> {
    let magick = if Command::new("magick").arg("-version").output().is_ok() {
        "magick"
    } else if Command::new("convert").arg("-version").output().is_ok() {
        "convert"
    } else {
        return Err("未找到 ImageMagick (magick/convert)，无法加水印".to_string());
    };

    // 按 max_size 选字号，保证占图比例稳定
    let pointsize = match max_size {
        0..=239 => 12,    // 240p（极快档）
        240..=359 => 18,  // 360p
        360..=539 => 22,  // 480p
        540..=899 => 30,  // 720p
        _ => 40,          // 1080p+
    };
    // 边距按字号缩放
    let pad = (pointsize / 2).max(8);
    // 跨平台找字体：优先用绝对路径，没有就回退到 "Arial"（ImageMagick 内置 fontconfig 会自己找）
    let font_arg = resolve_system_font().unwrap_or_else(|| "Arial".to_string());

    let total = frames.len();
    for (i, (t, path)) in frames.iter().enumerate() {
        let text = format!("t: {:.2}s  ({}/{})", t, i + 1, total);
        let status = Command::new(magick)
            .arg(path)
            .arg("-gravity").arg("NorthWest")
            .arg("-fill").arg("white")
            .arg("-undercolor").arg("rgba(0,0,0,0.6)")
            .arg("-pointsize").arg(pointsize.to_string())
            .arg("-font").arg(&font_arg)
            .arg("-annotate").arg(format!("+{}+{}", pad, pad))
            .arg(&text)
            .arg(path)
            .status()
            .map_err(|e| format!("magick 启动失败: {}", e))?;

        if !status.success() {
            eprintln!("watermark skip {}: exit {:?}", path, status.code());
        }
    }
    Ok(())
}
