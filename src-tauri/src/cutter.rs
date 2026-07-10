// 可乐裁切业务：
// - 启用 AI：抽帧 → AI 出切段方案 → 按方案切
// - 关闭 AI：纯裁剪 → 按每段固定时长切
use crate::config::AIConfig;
use crate::ffmpeg;
use crate::vision::{self, FrameShot};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::path::Path;
use tauri::Emitter;

fn log_to_file(msg: &str) {
    use std::io::Write;
    let path = std::env::var("HOME").unwrap_or_default() + "/cola-cutter-debug.log";
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), msg);
    }
}

macro_rules! dlog_cutter { ($($arg:tt)*) => { log_to_file(&format!($($arg)*)); } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutSegment {
    pub output_path: String,
    pub source: String,
    pub src_start: f64,
    pub src_end: f64,
    pub duration: f64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutResult {
    pub source_name: String,
    pub segments: Vec<CutSegment>,
    pub error: Option<String>,
}

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "flv", "m4v"];

#[tauri::command]
pub fn scan_folder(folder: &str) -> Result<Vec<VideoFile>, String> {
    let path = Path::new(folder);
    if !path.is_dir() {
        return Err(format!("不是有效文件夹: {}", folder));
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| format!("读取失败: {}", e))?.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        // 跳过 macOS 在外部存储上写的影子文件（如 ._xxx.mp4）和隐藏文件
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if name.starts_with("._") || (name.starts_with(".") && !name.starts_with("..")) { continue; }
        let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
        let Some(ext) = ext else { continue; };
        if !VIDEO_EXTS.contains(&ext.as_str()) { continue; }
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        out.push(VideoFile {
            path: p.to_string_lossy().to_string(),
            name,
            size: metadata.len(),
            duration: ffmpeg::probe_duration(&p.to_string_lossy()).ok(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[tauri::command]
pub fn get_video_duration(path: &str) -> Result<f64, String> {
    ffmpeg::probe_duration(path)
}

/// 用过的素材处理：none=不处理 / delete=删除 / move=放进子文件夹
#[tauri::command]
pub fn handle_used_videos(
    action: String,
    paths: Vec<String>,
    input_folder: String,
) -> Result<String, String> {
    dlog_cutter!("[handle_used_videos] action={}, count={}, input_folder={}", action, paths.len(), input_folder);
    if action == "none" || paths.is_empty() {
        return Ok("skipped".to_string());
    }

    if action == "move" {
        // 在输入文件夹下建一个 used 子文件夹
        let sub = Path::new(&input_folder).join("used");
        fs::create_dir_all(&sub).map_err(|e| format!("创建子文件夹失败: {}", e))?;
        let mut moved = 0usize;
        let mut failed = 0usize;
        for p in &paths {
            let src = Path::new(p);
            if !src.exists() { failed += 1; continue; }
            let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            let dst = sub.join(name);
            // 跨设备时 rename 会失败，自动降级为 copy+remove
            match fs::rename(src, &dst) {
                Ok(_) => moved += 1,
                Err(_) => {
                    match fs::copy(src, &dst) {
                        Ok(_) => {
                            let _ = fs::remove_file(src);
                            moved += 1;
                        }
                        Err(e) => {
                            dlog_cutter!("[handle_used_videos] move 失败: {} ({})", p, e);
                            failed += 1;
                        }
                    }
                }
            }
        }
        dlog_cutter!("[handle_used_videos] move 完成: moved={}, failed={}, subdir={:?}", moved, failed, sub);
        return Ok(format!("moved {} to {:?}", moved, sub));
    }

    if action == "delete" {
        let mut deleted = 0usize;
        let mut failed = 0usize;
        for p in &paths {
            match fs::remove_file(p) {
                Ok(_) => deleted += 1,
                Err(e) => {
                    dlog_cutter!("[handle_used_videos] delete 失败: {} ({})", p, e);
                    failed += 1;
                }
            }
        }
        dlog_cutter!("[handle_used_videos] delete 完成: deleted={}, failed={}", deleted, failed);
        return Ok(format!("deleted {}", deleted));
    }

    Err(format!("未知的 action: {}", action))
}

/// 纯裁剪命令（独立于 run_batch_cut，强制不走 AI）
#[tauri::command]
pub async fn run_pure_cut(
    app: tauri::AppHandle,
    input_folder: String,
    output_folder: String,
    target_duration: f64,
    group_by_video: bool,
    trim_last_short_segment: bool,
) -> Result<Vec<CutResult>, String> {
    dlog_cutter!("[run_pure_cut] 强制走纯裁剪, trim_last_short_segment={}", trim_last_short_segment);
    fs::create_dir_all(&output_folder).map_err(|e| format!("创建输出目录失败: {}", e))?;
    let videos = scan_folder(&input_folder)?;
    if videos.is_empty() {
        return Err("输入文件夹无视频".to_string());
    }

    let mut results = Vec::new();
    let total = videos.len();
    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": total, "item": "", "stage": "pure_start", "use_ai": false,
    }));

    for (idx, v) in videos.iter().enumerate() {
        let _ = app.emit("cut-progress", serde_json::json!({
            "current": idx + 1, "total": total, "item": v.name,
            "stage": "pure_cut", "use_ai": false,
        }));
        dlog_cutter!("[pure_cut] {}/{} name={}", idx + 1, total, v.name);

        let res = process_pure_cut_v2(&app, v, &output_folder, target_duration, group_by_video, trim_last_short_segment).await;
        results.push(res);
    }

    let _ = app.emit("cut-done", serde_json::json!({"total": total}));
    Ok(results)
}

/// AI 裁切命令（独立，强制走 AI）
#[tauri::command]
pub async fn run_ai_cut(
    app: tauri::AppHandle,
    input_folder: String,
    output_folder: String,
    config: AIConfig,
) -> Result<Vec<CutResult>, String> {
    dlog_cutter!("[run_ai_cut] 强制走 AI 裁切");
    fs::create_dir_all(&output_folder).map_err(|e| format!("创建输出目录失败: {}", e))?;
    let videos = scan_folder(&input_folder)?;
    if videos.is_empty() {
        return Err("输入文件夹无视频".to_string());
    }

    let mut results = Vec::new();
    let total = videos.len();
    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": total, "item": "", "stage": "start", "use_ai": true,
    }));

    for (idx, v) in videos.iter().enumerate() {
        let _ = app.emit("cut-progress", serde_json::json!({
            "current": idx + 1, "total": total, "item": v.name,
            "stage": "start", "use_ai": true,
        }));
        dlog_cutter!("[ai_cut] {}/{} name={}", idx + 1, total, v.name);
        let res = process_ai_mode(&app, v, &output_folder, &config).await;
        results.push(res);
    }

    let _ = app.emit("cut-done", serde_json::json!({"total": total}));
    Ok(results)
}

// =============== AI 模式 ===============

async fn process_ai_mode(
    app: &tauri::AppHandle,
    v: &VideoFile,
    output_folder: &str,
    config: &AIConfig,
) -> CutResult {
    let video_name = v.name.clone();
    let video_path = v.path.clone();

    let duration = match ffmpeg::probe_duration(&video_path) {
        Ok(d) => d,
        Err(e) => return CutResult {
            source_name: video_name, segments: vec![],
            error: Some(format!("探测时长失败: {}", e)),
        },
    };

    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "probe", "duration": duration,
    }));

    // ====== 分片处理（避免一次 AI 调用太大） ======
    let chunk_secs = if config.chunk_secs > 0.0 { config.chunk_secs } else { 120.0 };
    let chunks = split_into_time_chunks(duration, chunk_secs);
    dlog_cutter!("[{}] duration={:.1}s, chunk_secs={:.1}s, chunks={}", video_name, duration, chunk_secs, chunks.len());

    let tmp_dir = std::env::temp_dir().join(format!("cola_cut_{}", std::process::id()));
    let video_stem = Path::new(&video_name).file_stem().and_then(|s| s.to_str()).unwrap_or("v");

    let mut all_shots: Vec<crate::vision::CutShot> = Vec::new();
    let mut last_error: Option<String> = None;

    for (ci, (chunk_start, chunk_end)) in chunks.iter().enumerate() {
        let _ = app.emit("cut-progress", serde_json::json!({
            "current": 0, "total": 0, "item": video_name,
            "stage": "chunk_start", "chunk": ci + 1, "chunks": chunks.len(),
            "chunk_range": [chunk_start, chunk_end],
        }));

        let frames_dir = tmp_dir.join(format!("frames_{}_chunk{}", video_stem, ci));
        let _ = fs::create_dir_all(&frames_dir);

        let fps = if config.fps > 0.0 { config.fps } else { 1.0 };
        let scale = format!("scale={}:-2:flags=lanczos", config.frame_max_size);

        // ffmpeg 抽帧（带 -ss 和 -t，限制到当前分片）
        let frame_pattern = frames_dir.join("f_%04d.jpg");
        let extract_status = std::process::Command::new(ffmpeg::resolve_bin("ffmpeg"))
            .args([
                "-y", "-hide_banner", "-loglevel", "error",
                "-ss", &format!("{:.3}", chunk_start),
                "-i", &video_path,
                "-t", &format!("{:.3}", chunk_end - chunk_start),
                "-vf", &format!("fps={},{}", fps, scale),
                "-q:v", "5",
            ])
            .arg(&frame_pattern)
            .status();

        if let Ok(s) = extract_status {
            if !s.success() {
                last_error = Some(format!("分片 {} 抽帧失败", ci + 1));
                continue;
            }
        } else {
            last_error = Some(format!("分片 {} 无法启动 ffmpeg", ci + 1));
            continue;
        }

        // 加载帧
        let mut frame_files: Vec<(f64, String)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&frames_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if let Some(num_str) = name.strip_prefix("f_").and_then(|s| s.strip_suffix(".jpg")) {
                        if let Ok(n) = num_str.parse::<u32>() {
                            // 时间戳 = 抽帧在片段内的相对时间 + 片段起始偏移
                            let t = chunk_start + (n as f64 - 1.0) / fps;
                            frame_files.push((t, p.to_string_lossy().to_string()));
                        }
                    }
                }
            }
        }
        frame_files.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        if frame_files.is_empty() {
            let _ = fs::remove_dir_all(&frames_dir);
            continue;
        }

        // 加水印
        let _ = app.emit("cut-progress", serde_json::json!({
            "current": 0, "total": 0, "item": video_name,
            "stage": "watermarking", "chunk": ci + 1, "chunks": chunks.len(), "count": frame_files.len(),
        }));
        if let Err(e) = vision::watermark_frames(&frame_files, config.frame_max_size) {
            eprintln!("watermark warning: {}", e);
        }

        // 读 base64
        let mut frames: Vec<FrameShot> = Vec::new();
        for (t, path) in &frame_files {
            if let Ok(b64) = vision::read_file_base64(path) {
                frames.push(FrameShot { path: path.clone(), t: *t, data_base64: b64 });
            }
        }

        // AI 分析（传 chunk_offset，让 prompt 里说清时间偏移）
        let _ = app.emit("cut-progress", serde_json::json!({
            "current": 0, "total": 0, "item": video_name,
            "stage": "ai_analyzing", "chunk": ci + 1, "chunks": chunks.len(), "frames": frames.len(),
        }));

        match vision::ai_propose_segments(config, &config.requirement, &frames, chunk_end - chunk_start, *chunk_start).await {
            Ok(shots) => {
                dlog_cutter!("[{}] chunk {} AI 返回 {} shots", video_name, ci + 1, shots.len());
                let _ = app.emit("cut-progress", serde_json::json!({
                    "current": 0, "total": 0, "item": video_name,
                    "stage": "ai_done_chunk", "chunk": ci + 1, "chunks": chunks.len(), "valid": shots.len(),
                }));
                // 时间戳已经加过 chunk_offset（因为 frame 的 t 已经加了）
                all_shots.extend(shots);
            }
            Err(e) => {
                dlog_cutter!("chunk {} AI 失败: {}", ci + 1, e);
                last_error = Some(format!("分片 {} AI 失败: {}", ci + 1, e));
                // 继续下一个分片
            }
        }

        let _ = fs::remove_dir_all(&frames_dir);
    }

    let _ = fs::remove_dir_all(&tmp_dir);

    if all_shots.is_empty() {
        return CutResult { source_name: video_name, segments: vec![],
            error: Some(last_error.unwrap_or_else(|| "AI 未挑选出任何镜头".to_string())) };
    }

    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "ai_done", "valid_shots": all_shots.len(),
    }));

    // 切段
    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "cutting", "segments": all_shots.len(),
    }));

    let segments = cut_shots(app, &video_name, &video_path, output_folder, &all_shots, config.group_by_video).await;

    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "done", "segments": segments.len(),
    }));

    CutResult { source_name: video_name, segments, error: None }
}

/// 把总时长按 chunk_secs 切分（最后一节可能不足）
fn split_into_time_chunks(duration: f64, chunk_secs: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    if duration <= 0.0 || chunk_secs <= 0.0 {
        return vec![(0.0, duration.max(0.0))];
    }
    let mut t = 0.0;
    while t < duration {
        let end = (t + chunk_secs).min(duration);
        out.push((t, end));
        t = end;
    }
    out
}

// =============== 纯裁剪模式 ===============

/// 纯裁剪（不依赖 AIConfig，独立参数）
async fn process_pure_cut_v2(
    app: &tauri::AppHandle,
    v: &VideoFile,
    output_folder: &str,
    target_dur: f64,
    group_by_video: bool,
    trim_last: bool,
) -> CutResult {
    let video_name = v.name.clone();
    let video_path = v.path.clone();

    let duration = match ffmpeg::probe_duration(&video_path) {
        Ok(d) => d,
        Err(e) => return CutResult {
            source_name: video_name, segments: vec![],
            error: Some(format!("探测时长失败: {}", e)),
        },
    };

    let mut shots: Vec<(f64, f64, Option<String>)> = Vec::new();
    let mut cursor = 0.0;
    while cursor < duration {
        let end = cursor + target_dur;
        // 去尾段开关：开启时，最后一段不足 target_dur 就跳过不切
        if end > duration {
            if trim_last {
                break;
            } else {
                // 切完整：把剩余部分作为最后一段（时长会短于目标）
                shots.push((cursor, duration, None));
                break;
            }
        }
        shots.push((cursor, end, None));
        cursor = end;
    }

    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "pure_cutting", "segments": shots.len(),
    }));

    let shots_view: Vec<_> = shots.iter().map(|s| crate::vision::CutShot {
        start: s.0, end: s.1, reason: s.2.clone(),
    }).collect();

    let segments = cut_shots(app, &video_name, &video_path, output_folder, &shots_view, group_by_video).await;

    let _ = app.emit("cut-progress", serde_json::json!({
        "current": 0, "total": 0, "item": video_name,
        "stage": "done", "segments": segments.len(),
    }));

    CutResult { source_name: video_name, segments, error: None }
}

// =============== 共享切段函数 ===============

async fn cut_shots(
    _app: &tauri::AppHandle,
    video_name: &str,
    video_path: &str,
    output_folder: &str,
    shots: &[crate::vision::CutShot],
    group_by_video: bool,
) -> Vec<CutSegment> {
    let stem = Path::new(video_name).file_stem().and_then(|s| s.to_str()).unwrap_or("output");

    // 确定输出目录
    let out_dir = if group_by_video {
        let d = Path::new(output_folder).join(stem);
        let _ = fs::create_dir_all(&d);
        d.to_string_lossy().to_string()
    } else {
        output_folder.to_string()
    };

    let mut segments = Vec::new();
    for (i, shot) in shots.iter().enumerate() {
        // 文件名
        let fname = if group_by_video {
            format!("seg_{:03}.mp4", i + 1)
        } else {
            format!("{}_seg_{:03}.mp4", stem, i + 1)
        };
        let out_path = Path::new(&out_dir).join(&fname);
        let out_str = out_path.to_string_lossy().to_string();

        if let Ok(dur) = cut_one(video_path, shot.start, shot.end, &out_str).await {
            segments.push(CutSegment {
                output_path: out_str,
                source: video_name.to_string(),
                src_start: shot.start,
                src_end: shot.end,
                duration: dur,
                reason: shot.reason.clone(),
            });
        }
    }
    segments
}

async fn cut_one(src: &str, start: f64, end: f64, out_path: &str) -> Result<f64, String> {
    // v1.0.13：iPhone HEVC MOV 的 mebx 数据流会让 ffmpeg tessus 2018 hang（never exit），
    //         外层自动 fallback：失败 → remux（-c copy, 不重编码，秒级）→ 用 remux 后的 mp4 重试
    match cut_one_inner(src, start, end, out_path).await {
        Ok(d) => Ok(d),
        Err(e) => {
            dlog_cutter!("[cut_one] 第一次失败 ({}), 自动 remux 后重试", e);
            let remux_src = remux_for_safe_cut(src).await?;
            cut_one_inner(&remux_src, start, end, out_path).await
        }
    }
}

/// 把源文件 -c copy remux 到 ~/.cola-cutter-cache/remux/，去 mebx 数据流，供 cut_one 重试
/// v1.0.13：用 cache key（路径 hash + mtime）避免重复 remux
async fn remux_for_safe_cut(src: &str) -> Result<String, String> {
    let cache_dir = std::env::var("HOME").map(|h| format!("{}/.cola-cutter-cache/remux", h)).unwrap_or_else(|_| "/tmp/cola-cutter-cache/remux".to_string());
    fs::create_dir_all(&cache_dir).map_err(|e| format!("创建 cache 目录失败: {}", e))?;

    let key = format!("{:x}", md5_like_hash(src));
    let mtime = fs::metadata(src).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())).unwrap_or(0);
    let out_name = format!("{}_{}.mp4", key, mtime);
    let out_path = std::path::PathBuf::from(&cache_dir).join(&out_name);

    if out_path.exists() && fs::metadata(&out_path).map(|m| m.len() > 0).unwrap_or(false) {
        dlog_cutter!("[remux] cache 命中: {:?}", out_path);
        return Ok(out_path.to_string_lossy().to_string());
    }

    dlog_cutter!("[remux] {} -> {:?}", src, out_path);
    let out_str = out_path.to_string_lossy().to_string();
    let mut cmd = std::process::Command::new(ffmpeg::resolve_bin("ffmpeg"));
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-i", src, "-map", "0:v:0", "-map", "0:a?", "-c", "copy", "-movflags", "+faststart"])
        .arg(&out_str)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("remux 启动失败: {}", e))?;
    let status = tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(s)) => return Ok::<std::process::ExitStatus, String>(s),
                Ok(None) => {
                    if start.elapsed() > std::time::Duration::from_secs(180) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("remux 超时 180s".to_string());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }).await.map_err(|e| e.to_string())?;
    let s = status.map_err(|e| e)?;
    if !s.success() {
        let _ = fs::remove_file(&out_str);
        return Err(format!("remux exit={:?}", s.code()));
    }
    let size = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    if size < 1024 {
        let _ = fs::remove_file(&out_str);
        return Err(format!("remux 输出 0 字节"));
    }
    dlog_cutter!("[remux] OK {} bytes", size);
    Ok(out_str)
}

fn md5_like_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

async fn cut_one_inner(src: &str, start: f64, end: f64, out_path: &str) -> Result<f64, String> {
    let dur = end - start;
    // v1.0.6：tmp_path 加唯一后缀（时间戳+随机），避免并发 cut_one 覆盖同一个文件 + 不留 stale lock
    let tmp_name = format!(
        "cola_seg_{}_{}.mp4",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp_path = std::env::temp_dir().join(tmp_name);
    let tmp_str = tmp_path.to_string_lossy().to_string();
    dlog_cutter!("[cut_one_inner] start src={} [{:.2}-{:.2}] -> {}", src, start, end, out_path);

    // v1.0.11：回退 std::process::Command + process_group(0)，避免 tokio + tauri NSApp SIGCHLD 竞争
    //         （macOS posix_spawn Assertion failed (advance > 0)）
    let mut cmd = std::process::Command::new(ffmpeg::resolve_bin("ffmpeg"));
    cmd.args([
        "-y", "-hide_banner", "-loglevel", "error",
        "-ss", &format!("{:.3}", start),
        "-i", src,
        "-t", &format!("{:.3}", dur),
        "-c:v", "libx264",
        "-preset", "ultrafast",
        "-crf", "23",
        "-c:a", "aac",
        "-b:a", "128k",
        "-movflags", "+faststart",
    ])
    .arg(&tmp_str)
    .process_group(0)  // ← setsid，让子进程脱离父进程组 + session，独立 SIGCHLD
    .stdout(Stdio::null())
    .stderr(Stdio::null()); // v1.0.12：不能用 piped，没人读会撑爆 pipe buffer 让 ffmpeg hang

    let mut child = cmd.spawn().map_err(|e| format!("启动 ffmpeg 失败: {}", e))?;
    dlog_cutter!("[cut_one_inner] ffmpeg 启动, pid={}", child.id());

    // 用 spawn_blocking 同步等（不阻塞 tokio runtime）+ 超时 kill 兜底
    let timeout = std::time::Duration::from_secs_f64(dur + 60.0);
    let wait_result = tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok::<std::process::ExitStatus, String>(status),
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("ffmpeg 超时（>{:.0}s）被强杀", timeout.as_secs_f64()));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(format!("try_wait 失败: {}", e)),
            }
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking 失败: {}", e))?;

    let status = match wait_result {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_file(&tmp_str);
            return Err(e);
        }
    };

    if !status.success() {
        let _ = fs::remove_file(&tmp_str);
        dlog_cutter!("[cut_one_inner] ffmpeg exit={:?}", status.code());
        return Err(format!("ffmpeg 切段失败 (exit: {:?})", status.code()));
    }

    // v1.0.12：检查文件大小，0 字节 = ffmpeg 假成功 / 提前退出
    let out_size = fs::metadata(&tmp_str).map(|m| m.len()).unwrap_or(0);
    if out_size < 1024 {
        let _ = fs::remove_file(&tmp_str);
        dlog_cutter!("[cut_one_inner] 输出 0 字节！");
        return Err(format!("ffmpeg 切段输出 0 字节（src={} [{:.2}-{:.2}]）", src, start, end));
    }

    if let Err(e) = fs::rename(&tmp_str, out_path) {
        fs::copy(&tmp_str, out_path).map_err(|e2| format!("copy 也失败: {} / {}", e, e2))?;
        let _ = fs::remove_file(&tmp_str);
    }
    dlog_cutter!("[cut_one_inner] OK {} bytes", out_size);
    Ok(dur)
}
