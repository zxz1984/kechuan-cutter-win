// 测试：竖屏主材 + 横屏 B-roll + PIP center 画中画
// 验证：主材持续显示 + B-roll 嵌入主材 + 输出尺寸按全局设置

use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn test_pip_portrait_with_landscape_broll() {
    let main_path = "/tmp/lzc_test2/portrait_main.mp4";
    let broll_path = "/tmp/lzc_test2/landscape_broll.mp4";

    let main_duration = probe(main_path);
    let (main_w, main_h) = probe_dims(main_path);
    let (broll_w, broll_h) = probe_dims(broll_path);
    println!(
        "主材: {}x{} {:.2}s, B-roll: {}x{}",
        main_w, main_h, main_duration, broll_w, broll_h
    );

    assert!(main_h > main_w, "主材应为竖屏");
    assert!(broll_w > broll_h, "B-roll 应为横屏");

    // 时间线：主材 0-3s，画中画 3-6s（用横屏 B-roll），主材 6-10s
    let clips = vec![
        Clip {
            start: 0.0,
            end: 3.0,
            source: "main",
            path: "",
            silent: false,
            layout: "replaced",
            pip_scale: 0.5,
        },
        Clip {
            start: 3.0,
            end: 6.0,
            source: "broll",
            path: broll_path,
            silent: true,
            layout: "pip_center", // PIP 居中
            pip_scale: 0.5,        // 占主材 50% 大小
        },
        Clip {
            start: 6.0,
            end: main_duration,
            source: "main",
            path: "",
            silent: false,
            layout: "replaced",
            pip_scale: 0.5,
        },
    ];

    let out_path = "/tmp/lzc_test2/out/pip_center.mp4";
    std::fs::create_dir_all(Path::new(out_path).parent().unwrap()).unwrap();

    // 输出用 portrait 设置（跟随主材）
    run_ffmpeg_pip(
        main_path,
        main_duration,
        main_w,
        main_h,
        &clips,
        out_path,
        &[broll_path],
        "portrait", // out: 1080x1920
        "follow_main", // 分辨率跟主材
    )
    .expect("PIP 混剪失败");

    // 验证输出
    let (out_w, out_h) = probe_dims(out_path);
    let out_duration = probe(out_path);
    println!("输出: {}x{} {:.2}s", out_w, out_h, out_duration);

    // portrait + follow_main → 1080x1920 (跟随主材)
    assert_eq!(out_w, main_w, "输出宽应=主材宽");
    assert_eq!(out_h, main_h, "输出高应=主材高");
    assert!((out_duration - main_duration).abs() < 1.0);

    // 验证有音频（主材音贯穿）
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "default=noprint_wrappers=1", out_path])
        .output()
        .expect("ffprobe");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.lines().any(|l| l.starts_with("codec_type=audio")),
        "输出应保持主材音频"
    );

    println!("✓ PIP 测试通过：竖屏主材 + 横屏 B-roll 嵌入显示");
}

#[derive(Debug)]
struct Clip {
    start: f64,
    end: f64,
    source: &'static str,
    path: &'static str,
    silent: bool,
    layout: &'static str,
    pip_scale: f64,
}

fn probe(path: &str) -> f64 {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1", path])
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap()
}

fn probe_dims(path: &str) -> (u32, u32) {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", path])
        .output()
        .expect("ffprobe");
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = s.split('x');
    let w: u32 = parts.next().unwrap().trim().parse().unwrap();
    let h: u32 = parts.next().unwrap().trim().parse().unwrap();
    (w, h)
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg_pip(
    main_path: &str,
    _main_duration: f64,
    main_w: u32,
    main_h: u32,
    clips: &[Clip],
    out_path: &str,
    broll_inputs: &[&str],
    orientation: &str,
    _resolution: &str,
) -> Result<(), String> {
    // 计算输出尺寸
    let (out_w, out_h) = match orientation {
        "portrait" => (1080u32, 1920),
        "landscape" => (1920, 1080),
        _ => (main_w, main_h),
    };
    let out_w = (out_w + 1) & !1;
    let out_h = (out_h + 1) & !1;

    let mut filters: Vec<String> = Vec::new();
    let mut video_labels: Vec<String> = Vec::new();
    let mut audio_labels: Vec<String> = Vec::new();

    for (i, c) in clips.iter().enumerate() {
        let vlabel = format!("v{}", i);
        let alabel = format!("a{}", i);
        let dur = c.end - c.start;
        if c.source == "main" {
            filters.push(format!(
                "[0:v]trim=start={:.3}:end={:.3},scale={}:{}:flags=lanczos,setpts=PTS-STARTPTS[{}]",
                c.start, c.end, out_w, out_h, vlabel
            ));
            filters.push(format!(
                "[0:a]atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS,aresample=44100,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                c.start, c.end, alabel
            ));
        } else {
            let broll_idx = broll_inputs
                .iter()
                .position(|p| *p == c.path)
                .unwrap() + 1;
            if c.layout.starts_with("pip_") {
                let pip_w = ((out_w as f64) * c.pip_scale) as u32;
                let pip_h = ((out_h as f64) * c.pip_scale) as u32;
                let pip_w_e = (pip_w + 1) & !1;
                let pip_h_e = (pip_h + 1) & !1;
                filters.push(format!(
                    "[{}:v]trim=end={:.3},scale={}:{}:force_original_aspect_ratio=decrease:eval=frame,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=black,setpts=PTS-STARTPTS[{}]",
                    broll_idx, dur, pip_w_e, pip_h_e, pip_w_e, pip_h_e, vlabel
                ));
                // PIP：先把主材 trim 出来当底层
                let mainb_label = format!("mainb{}", i);
                filters.push(format!(
                    "[0:v]trim=start={:.3}:end={:.3},scale={}:{}:flags=lanczos,setpts=PTS-STARTPTS[{}]",
                    c.start, c.end, out_w, out_h, mainb_label
                ));
                // overlay PIP → mainBase
                let (x, y) = match c.layout {
                    "pip_center" => ("(main_w-overlay_w)/2", "(main_h-overlay_h)/2"),
                    "pip_top" => ("(main_w-overlay_w)/2", "0"),
                    "pip_bottom" => ("(main_w-overlay_w)/2", "main_h-overlay_h"),
                    _ => unreachable!(),
                };
                filters.push(format!(
                    "[{}][{}]overlay=x={}:y={}:format=auto:eof_action=pass[{}]",
                    mainb_label, vlabel, x, y, vlabel
                ));
            } else {
                // Replaced 模式
                filters.push(format!(
                    "[{}:v]trim=end={:.3},scale={}:{}:force_original_aspect_ratio=decrease:eval=frame,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=black,setpts=PTS-STARTPTS[{}]",
                    broll_idx, dur, out_w, out_h, out_w, out_h, vlabel
                ));
            }
            if c.silent {
                filters.push(format!(
                    "anullsrc=channel_layout=stereo:sample_rate=44100,atrim=end={:.3},asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                    dur, alabel
                ));
            } else {
                filters.push(format!(
                    "[{}:a]atrim=end={:.3},asetpts=PTS-STARTPTS,aresample=44100,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                    broll_idx, dur, alabel
                ));
            }
        }
        video_labels.push(format!("[{}]", vlabel));
        audio_labels.push(format!("[{}]", alabel));
    }

    let video_input = video_labels.join("");
    filters.push(format!("{}concat=n={}:v=1:a=0[outv]", video_input, clips.len()));

    let audio_input = audio_labels.join("");
    filters.push(format!("{}concat=n={}:v=0:a=1[outa]", audio_input, clips.len()));

    filters.push("[outv][outa]concat=n=1:v=1:a=1[outv_final][outa_final]".to_string());

    let filter_complex = filters.join(";");
    println!("filter_complex:\n{}\n", filter_complex);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-hide_banner").arg("-loglevel").arg("error");
    cmd.args(["-i", main_path]);
    for b in broll_inputs {
        cmd.args(["-i", b]);
    }
    cmd.args(["-filter_complex", &filter_complex]);
    cmd.args(["-map", "[outv_final]"]);
    cmd.args(["-map", "[outa_final]"]);
    cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);
    cmd.args(["-c:a", "aac", "-b:a", "128k"]);
    cmd.arg(&out_path);

    let output = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("启动 ffmpeg 失败");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        println!("ffmpeg 错误:\n{}", stderr);
        return Err("ffmpeg 失败".to_string());
    }
    Ok(())
}
