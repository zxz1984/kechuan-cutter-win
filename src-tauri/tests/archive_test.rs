// 验证 archive 子命令
// 跑：cargo test --test archive_test -- --nocapture

use std::fs;
use std::path::Path;

#[path = "../src/archive.rs"]
mod archive;

fn setup_dir() -> String {
    let dir = format!("/tmp/lzc_archive_test_{}", std::process::id());
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_archive_to_subfolder() {
    let dir = setup_dir();
    // 创建 3 个测试文件
    let f1 = format!("{}/video1.mp4", dir);
    let f2 = format!("{}/video2.mp4", dir);
    let f3 = format!("{}/video3.mp4", dir);
    fs::write(&f1, b"fake1").unwrap();
    fs::write(&f2, b"fake2").unwrap();
    fs::write(&f3, b"fake3").unwrap();

    // 归档
    let res = archive::archive_to_subfolder(vec![f1.clone(), f2.clone(), f3.clone()]);
    assert!(res.is_ok(), "归档失败: {:?}", res);
    let moved = res.unwrap();
    assert_eq!(moved, 3, "应该移动 3 个文件");

    // 原文件应该不存在
    assert!(!Path::new(&f1).exists());
    assert!(!Path::new(&f2).exists());
    assert!(!Path::new(&f3).exists());

    // .used/ 子文件夹里应该有 3 个文件
    let used_dir = format!("{}/.used", dir);
    assert!(Path::new(&used_dir).is_dir(), ".used/ 子文件夹未创建");
    let moved_files: Vec<String> = fs::read_dir(&used_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(moved_files.contains(&"video1.mp4".to_string()));
    assert!(moved_files.contains(&"video2.mp4".to_string()));
    assert!(moved_files.contains(&"video3.mp4".to_string()));

    println!("✓ archive_to_subfolder 测试通过");

    // 二次归档——加文件名防冲突
    fs::write(&f1, b"fake1_again").unwrap();
    let res2 = archive::archive_to_subfolder(vec![f1.clone()]);
    assert!(res2.is_ok());
    let moved_files2: Vec<String> = fs::read_dir(&used_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(moved_files2.len() == 4, "第二次归档应自动重命名，得到 4 个文件");

    // 清理
    fs::remove_dir_all(&dir).unwrap();
    println!("✓ 重名归档测试通过");
}
