// 测试 runWith 的逻辑
const DEFAULT_CONFIG = {
  useAi: false,
  requirement: { targetDuration: 5, tolerance: 1 },
};

function testRunWith(config, useAi) {
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

// 测试 1: 默认纯裁切
let cfg = { ...DEFAULT_CONFIG };
let r1 = testRunWith(cfg, cfg.useAi);
console.log('测试 1 (默认纯裁切):', r1.useAi === false ? '✅' : '❌', 'useAi =', r1.useAi);

// 测试 2: 切到 AI 后点开始
cfg = { ...DEFAULT_CONFIG, useAi: true };
let r2 = testRunWith(cfg, cfg.useAi);
console.log('测试 2 (AI 裁切):', r2.useAi === true ? '✅' : '❌', 'useAi =', r2.useAi);

// 测试 3: 切到纯裁切后点开始
cfg = { ...DEFAULT_CONFIG, useAi: false };
let r3 = testRunWith(cfg, cfg.useAi);
console.log('测试 3 (纯裁切):', r3.useAi === false ? '✅' : '❌', 'useAi =', r3.useAi);
