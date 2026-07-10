// 模拟前端调用 Tauri 的完整流程
const { exec } = require('child_process');
const path = require('path');

// 模拟前端 React 组件的 runWith 函数
function runWith(config, useAi) {
  const finalConfig = {
    ...config,
    useAi: useAi,  // 强制覆盖
    requirement: {
      ...config.requirement,
      targetDuration: 5,
      tolerance: 1,
    },
  };
  return finalConfig;
}

// 模拟 Tauri 调用序列化（camelCase -> snake_case）
function toRustConfig(jsConfig) {
  return {
    base_url: jsConfig.baseUrl || '',
    api_key: jsConfig.apiKey || '',
    model: jsConfig.model || '',
    fps: jsConfig.fps || 1,
    frame_max_size: jsConfig.frameMaxSize || 480,
    use_ai: jsConfig.useAi === true,  // 关键
    group_by_video: jsConfig.groupByVideo || false,
    requirement: {
      fields: jsConfig.requirement?.fields || [],
      target_duration: jsConfig.requirement?.targetDuration || 5,
      tolerance: jsConfig.requirement?.tolerance || 1,
    },
  };
}

console.log('=== 场景 1：默认（纯裁切）===');
let cfg = {
  baseUrl: '', apiKey: '', model: '',
  fps: 1, frameMaxSize: 480, useAi: false, groupByVideo: false,
  requirement: { fields: [], targetDuration: 5, tolerance: 1 },
};
let r = runWith(cfg, cfg.useAi);  // 用户点开始，按钮调用 runWith(config.useAi) = runWith(false)
let rust = toRustConfig(r);
console.log('前端 useAi =', r.useAi);
console.log('后端 use_ai =', rust.use_ai);
console.log('走纯裁切?', rust.use_ai === false ? '✅ 正确' : '❌ 错误');
console.log('');

console.log('=== 场景 2：用户切到 AI 裁切后点开始 ===');
cfg.useAi = true;
r = runWith(cfg, cfg.useAi);
rust = toRustConfig(r);
console.log('前端 useAi =', r.useAi);
console.log('后端 use_ai =', rust.use_ai);
console.log('走 AI 裁切?', rust.use_ai === true ? '✅ 正确' : '❌ 错误');
console.log('');

console.log('=== 场景 3：用户切回纯裁切后点开始 ===');
cfg.useAi = false;
r = runWith(cfg, cfg.useAi);
rust = toRustConfig(r);
console.log('前端 useAi =', r.useAi);
console.log('后端 use_ai =', rust.use_ai);
console.log('走纯裁切?', rust.use_ai === false ? '✅ 正确' : '❌ 错误');
