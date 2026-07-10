#!/bin/bash
# 直接读 Rust 代码中的 use_ai 检查逻辑
echo "=== 检查 Rust 端 use_ai 处理 ==="
grep -n "use_ai" /Users/zxz/Documents/trae_projects/zxz/cola-cutter/src-tauri/src/cutter.rs
echo ""
echo "=== 检查 process_ai_mode 入口 ==="
grep -A 5 "let res = if use_ai" /Users/zxz/Documents/trae_projects/zxz/cola-cutter/src-tauri/src/cutter.rs
echo ""
echo "=== 检查 process_pure_cut 入口 ==="
grep -n "process_pure_cut" /Users/zxz/Documents/trae_projects/zxz/cola-cutter/src-tauri/src/cutter.rs
