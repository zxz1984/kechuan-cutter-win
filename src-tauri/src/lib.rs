// 可乐裁切 - 库入口
mod config;
mod cutter;
mod ffmpeg;
mod vision;

pub use config::{
    delete_template, list_templates, load_ai_config, save_ai_config, save_template,
    AIConfig, CutTemplate, ReqField, TemplateList,
};
pub use cutter::{get_video_duration, handle_used_videos, run_pure_cut, run_ai_cut, scan_folder, CutResult, CutSegment};
pub use vision::test_connection;

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use tauri::Manager;

/// v1.0.14：启动时杀掉所有残留的 cola-cutter 实例和 ffmpeg 子进程，清掉 tmp 切段文件
///
/// 设计：
/// 1. 只在 dev 模式（debug_assertions）跑，避免影响生产 .app 用户
/// 2. 跳过自己 PID（不然自杀）
/// 3. 杀 cola-cutter 之前先 pkill 它所有的子进程（ffmpeg），不然变 orphan
/// 4. 清 cola_seg_*.mp4 tmp 文件，下次启动不会读到半截残文件
fn startup_cleanup() {
    #[cfg(debug_assertions)]
    {
        let me = std::process::id();
        log_to_cleanup(&format!("[startup_cleanup] 自己 PID={}", me));

        // 1. 找所有 target/debug/cola-cutter 实例
        if let Ok(out) = std::process::Command::new("pgrep")
            .args(["-f", "target/debug/cola-cutter"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    if pid == me {
                        continue;
                    }
                    // 先杀它所有子进程（ffmpeg），避免变 orphan 继续占文件锁
                    let _ = std::process::Command::new("pkill")
                        .args(["-P", &pid.to_string()])
                        .output();
                    // 再杀主进程
                    let _ = std::process::Command::new("kill")
                        .arg("-TERM")
                        .arg(&pid.to_string())
                        .output();
                    log_to_cleanup(&format!("[startup_cleanup] 已 kill 老 cola-cutter PID={}", pid));
                }
            }
        }

        // 2. 清残留 cola_seg_*.mp4 tmp 文件
        if let Ok(dir) = std::env::temp_dir().read_dir() {
            for e in dir.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if name.starts_with("cola_seg_") && name.ends_with(".mp4") {
                        if let Err(err) = std::fs::remove_file(e.path()) {
                            log_to_cleanup(&format!("[startup_cleanup] 删 tmp 失败: {} ({})", name, err));
                        }
                    }
                }
            }
        }

        // 3. 等老进程体面退出（最多 2s），强杀兜底
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn log_to_cleanup(msg: &str) {
    use std::io::Write;
    let path = std::env::var("HOME").unwrap_or_default() + "/cola-cutter-debug.log";
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), msg);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            // v1.0.14：启动时清理 — kill 老的 cola-cutter 实例 + 残留 ffmpeg 子进程 + tmp 文件
            // 解决用户痛点：切段卡死后必须手动 kill 才能再用
            startup_cleanup();
            // dev 模式：先 hide 窗口，等 vite 1420 端口就绪再 show（避免白屏）
            // build 模式：跳过这步，窗口默认 visible:true 直接显示
            #[cfg(debug_assertions)]
            {
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.hide();
                }
                std::thread::spawn(move || {
                    let addr: std::net::SocketAddr = match "127.0.0.1:1420".to_socket_addrs() {
                        Ok(mut it) => match it.next() {
                            Some(a) => a,
                            None => return,
                        },
                        Err(_) => return,
                    };
                    for _ in 0..150 {
                        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                            if let Some(w) = handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_folder,
            get_video_duration,
            run_pure_cut,
            run_ai_cut,
            handle_used_videos,
            load_ai_config,
            save_ai_config,
            list_templates,
            save_template,
            delete_template,
            test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
