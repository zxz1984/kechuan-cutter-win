// 老郑混剪 - 集成测试（验证时间线 + silent 模式）
// 跑：cargo test --test integration_test -- --nocapture

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn test_real_mix_with_timeline() {
    let main_path = "/tmp/lzc_test/main/zb01.mp4";

    let main_duration = probe(main_path);
    let (main_w, main_h) = probe_dims(main_path);
    println!("主材时长: {:.2}s, 尺寸: {}x{}", main_duration, main_w, main_h);

    // B-roll 5s 时长 — broll_with_audio 实际有声音，broll1/a01 没声音
    let broll_durations: HashMap<String, f64> = [
        (
            "/tmp/lzc_test/broll1/a01.mp4".to_string(),
            probe("/tmp/lzc_test/broll1/a01.mp4"),
        ),
        (
            "/tmp/lzc_test/broll_with_audio/c01.mp4".to_string(),
            probe("/tmp/lzc_test/broll_with_audio/c01.mp4"),
        ),
    ]
    .into_iter()
    .collect();

    // 时间线 + silent 标记
    let clips = vec![
        Clip {
            start: 0.0,
            end: 5.0,
            source: "main",
            path: "",
            silent: false,
        },
        Clip {
            start: 5.0,
            end: 8.0,
            source: "broll",
            path: "/tmp/lzc_test/broll1/a01.mp4",
            silent: true,
        }, // 强制静音
        Clip {
            start: 8.0,
            end: 15.0,
            source: "main",
            path: "",
            silent: false,
        },
        Clip {
            start: 15.0,
            end: 20.0,
            source: "broll",
            path: "/tmp/lzc_test/broll_with_audio/c01.mp4",
            silent: false,
        }, // 用素材原声（c01 有声音）
        Clip {
            start: 20.0,
            end: main_duration,
            source: "main",
            path: "",
            silent: false,
        },
    ];

    let out_path = "/tmp/lzc_test/out/zb01_mixed.mp4";
    std::fs::create_dir_all(Path::new(out_path).parent().unwrap()).unwrap();

    run_ffmpeg_concat(main_path, main_duration, main_w, main_h, &clips, &broll_durations, out_path, &[
        "/tmp/lzc_test/broll1/a01.mp4",
        "/tmp/lzc_test/broll_with_audio/c01.mp4",
    ])
    .expect("混剪失败");

    assert!(Path::new(out_path).exists(), "输出文件未生成");
    let out_duration = probe(out_path);
    println!("输出时长: {:.2}s", out_duration);
    let diff = (out_duration - main_duration).abs();
    assert!(
        diff < 1.0,
        "时长偏差过大: {:.2} vs {:.2}",
        out_duration,
        main_duration
    );

    println!("✓ 集成测试通过 (silent + 有声混合)");
}

#[derive(Debug)]
#[allow(dead_code)]
struct Clip {
    start: f64,
    end: f64,
    source: &'static str,
    path: &'static str,
    silent: bool,
}

fn probe(path: &str) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path,
        ])
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap()
}

fn probe_dims(path: &str) -> (u32, u32) {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height",
            "-of", "csv=p=0:s=x",
            path,
        ])
        .output()
        .expect("ffprobe");
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = s.split('x');
    let w: u32 = parts.next().unwrap().trim().parse().unwrap();
    let h: u32 = parts.next().unwrap().trim().parse().unwrap();
    (w, h)
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg_concat(
    main_path: &str,
    _main_duration: f64,
    main_w: u32,
    main_h: u32,
    clips: &[Clip],
    broll_durations: &HashMap<String, f64>,
    out_path: &str,
    broll_inputs: &[&str],
) -> Result<(), String> {
    let mw = main_w;
    let mh = main_h;
    let mw_even = (mw + 1) & !1;
    let mh_even = (mh + 1) & !1;

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
                c.start, c.end, mw_even, mh_even, vlabel
            ));
            filters.push(format!(
                "[0:a]atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS,aresample=44100,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                c.start, c.end, alabel
            ));
        } else {
            let broll_idx = broll_inputs
                .iter()
                .position(|p| *p == c.path)
                .ok_or_else(|| format!("找不到 broll: {}", c.path))?
                + 1;
            let broll_dur = broll_durations.get(c.path).copied().unwrap_or(dur);
            let take = dur.min(broll_dur);
            filters.push(format!(
                "[{}:v]trim=end={:.3},scale={}:{}:force_original_aspect_ratio=decrease:eval=frame,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=black,setpts=PTS-STARTPTS[{}]",
                broll_idx, take, mw_even, mh_even, mw_even, mh_even, vlabel
            ));
            if c.silent {
                filters.push(format!(
                    "anullsrc=channel_layout=stereo:sample_rate=44100,atrim=end={:.3},asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                    take, alabel
                ));
            } else {
                filters.push(format!(
                    "[{}:a]atrim=end={:.3},asetpts=PTS-STARTPTS,aresample=44100,aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo[{}]",
                    broll_idx, take, alabel
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
