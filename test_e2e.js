// 端到端测试：模拟前端调 Tauri 命令，验证 use_ai 字段传递
// 这模拟 Vite 编译出的 JS 在 invoke('run_batch_cut', { config }) 时会发生什么

// Tauri 2.x 默认将 camelCase 转为 snake_case
function toSnakeCase(obj) {
  if (obj === null || typeof obj !== 'object') return obj;
  if (Array.isArray(obj)) return obj.map(toSnakeCase);
  const result = {};
  for (const [key, value] of Object.entries(obj)) {
    const snakeKey = key.replace(/[A-Z]/g, (m) => '_' + m.toLowerCase());
    result[snakeKey] = toSnakeCase(value);
  }
  return result;
}

// 模拟前端传纯裁切
const frontendConfig1 = {
  useAi: false,  // 前端 camelCase
  requirement: { targetDuration: 5, tolerance: 1 },
};
const tauriConfig1 = toSnakeCase(frontendConfig1);
console.log('【纯裁切】前端→Tauri:', JSON.stringify(tauriConfig1));
console.log('  Rust 收到 use_ai =', tauriConfig1.use_ai);
console.log('  走 process_pure_cut?', tauriConfig1.use_ai === false ? '✅' : '❌');
console.log('');

// 模拟前端传 AI 裁切
const frontendConfig2 = {
  useAi: true,
  requirement: { targetDuration: 5, tolerance: 1 },
};
const tauriConfig2 = toSnakeCase(frontendConfig2);
console.log('【AI 裁切】前端→Tauri:', JSON.stringify(tauriConfig2));
console.log('  Rust 收到 use_ai =', tauriConfig2.use_ai);
console.log('  走 process_ai_mode?', tauriConfig2.use_ai === true ? '✅' : '❌');
