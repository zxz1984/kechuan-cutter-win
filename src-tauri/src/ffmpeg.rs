// FFmpeg 工具：路径解析 + 探测时长/分辨率
// 可乐裁切只用到这些基础函数
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Serialize, Clone)]
pub struct FfmpegLog {
    pub line: String,
    pub stream: String,
}

/// 解析 ffmpeg 的绝对路径
///
/// 查找顺序：
/// 0. 环境变量 `COLA_CUTTER_FFMPEG`（最高优先级，强制覆盖）
/// 1. Tauri bundle 的 Resources/ 目录（macOS）或 exe 同目录（Windows）—— 离线优先
/// 2. 项目本地 `bin/ffmpeg-darwin-x64`（dev 模式关键 —— 避免 brew ffmpeg 兼容性问题）
/// 3. `which` 系统 PATH
/// 4. fallback 常见路径 /opt/homebrew/bin, /usr/local/bin, /usr/bin
///
/// v1.0.5 新增：环境变量 + 项目本地 bin/，避免 dev 模式误用 brew ffmpeg 卡死
pub fn resolve_bin(name: &str) -> String {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        let mut m = HashMap::new();
        let bin = if name.is_empty() { "ffmpeg" } else { name };
        let mut found: Option<String> = None;

        // 0) 环境变量强制（用户可手动覆盖）
        //    用法：export COLA_CUTTER_FFMPEG=/path/to/your/ffmpeg
        if found.is_none() {
            if let Ok(env_bin) = std::env::var("COLA_CUTTER_FFMPEG") {
                let p = env_bin.trim();
                if !p.is_empty() && std::path::Path::new(p).exists() {
                    if let Ok(canon) = std::path::Path::new(p).canonicalize() {
                        found = Some(canon.to_string_lossy().to_string());
                    } else {
                        found = Some(p.to_string());
                    }
                }
            }
        }

        // 1) Tauri bundle 内（生产 .app 模式）
        if found.is_none() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let candidates = [
                        dir.join(format!("../Resources/{}", bin)),
                        dir.join(format!("../Resources/_up_/bin/{}", bin)),
                        dir.join(format!("Resources/{}", bin)),
                        dir.join(bin),
                    ];
                    for c in &candidates {
                        if c.exists() {
                            if let Ok(canon) = c.canonicalize() {
                                found = Some(canon.to_string_lossy().to_string());
                            } else {
                                found = Some(c.to_string_lossy().to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }

        // 2) 项目本地 bin/（dev 模式关键）
        //    exe 路径：src-tauri/target/debug/cola-cutter
        //    往上 3 层 → 项目根 → bin/ffmpeg-darwin-x64
        if found.is_none() {
            if let Ok(exe) = std::env::current_exe() {
                // ancestors(): [cola-cutter, debug, target, src-tauri, project_root]
                // 第 4 个是项目根
                if let Some(project_root) = exe.ancestors().nth(4) {
                    for sub in ["bin/ffmpeg-darwin-x64", "bin/ffmpeg"] {
                        let local = project_root.join(sub);
                        if local.exists() {
                            if let Ok(canon) = local.canonicalize() {
                                found = Some(canon.to_string_lossy().to_string());
                            } else {
                                found = Some(local.to_string_lossy().to_string());
                            }
                            break;
                        }
                    }
                }
                // 兜底：cwd 的 bin/
                if found.is_none() {
                    if let Ok(cwd) = std::env::current_dir() {
                        let local = cwd.join("bin").join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
                        if local.exists() {
                            if let Ok(canon) = local.canonicalize() {
                                found = Some(canon.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        // 3) which 系统 PATH
        if found.is_none() {
            if let Ok(out) = std::process::Command::new("which").arg(bin).output() {
                if out.status.success() {
                    if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                        let p = line.trim().to_string();
                        if std::path::Path::new(&p).exists() {
                            found = Some(p);
                        }
                    }
                }
            }
        }

        // 4) 常见路径 fallback
        if found.is_none() {
            for p in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
                let cand = format!("{}/{}", p, bin);
                if std::path::Path::new(&cand).exists() {
                    found = Some(cand);
                    break;
                }
            }
        }

        if let Some(p) = found {
            m.insert(bin.to_string(), p);
        }
        m
    });
    cache
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// 用 ffmpeg -i 查时长（兼容没 ffprobe 的情况）
pub fn probe_duration(path: &str) -> Result<f64, String> {
    let out = std::process::Command::new(resolve_bin("ffmpeg"))
        .args(["-hide_banner", "-i", path, "-t", "0", "-f", "null", "-"])
        .output()
        .map_err(|e| format!("ffmpeg 启动失败: {}", e))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if let Some(idx) = line.find("Duration:") {
            let after = &line[idx + 9..];
            let trimmed = after.trim_start();
            if let Some(comma_idx) = trimmed.find(',') {
                let time_str = trimmed[..comma_idx].trim();
                let parts: Vec<&str> = time_str.split(':').collect();
                if parts.len() == 3 {
                    let h: f64 = parts[0].parse().unwrap_or(0.0);
                    let m: f64 = parts[1].parse().unwrap_or(0.0);
                    let s: f64 = parts[2].parse().unwrap_or(0.0);
                    return Ok(h * 3600.0 + m * 60.0 + s);
                }
            }
        }
    }
    Err("无法从 ffmpeg 输出解析时长".to_string())
}

/// 用 ffmpeg -i 查分辨率
pub fn probe_dimensions(path: &str) -> Result<(u32, u32), String> {
    let out = std::process::Command::new(resolve_bin("ffmpeg"))
        .args(["-hide_banner", "-i", path, "-t", "0", "-f", "null", "-"])
        .output()
        .map_err(|e| format!("ffmpeg 启动失败: {}", e))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if line.contains("Video:") {
            if let Some(start) = line.find(' ') {
                let rest = &line[start..];
                for word in rest.split(',') {
                    let w = word.trim();
                    if let Some(x_pos) = w.find('x') {
                        if w.chars().take(x_pos).all(|c| c.is_ascii_digit())
                            && w.chars().skip(x_pos + 1).take_while(|c| c.is_ascii_digit()).count() > 0
                        {
                            let w_str = &w[..x_pos];
                            let mut chars_after = w[x_pos + 1..].chars();
                            let mut h_str = String::new();
                            while let Some(c) = chars_after.next() {
                                if c.is_ascii_digit() {
                                    h_str.push(c);
                                } else {
                                    break;
                                }
                            }
                            if let (Ok(w), Ok(h)) = (w_str.parse::<u32>(), h_str.parse::<u32>()) {
                                return Ok((w, h));
                            }
                        }
                    }
                }
            }
        }
    }
    Err("无法从 ffmpeg 输出解析分辨率".to_string())
}
