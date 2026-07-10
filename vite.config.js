import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({
    plugins: [react()],
    clearScreen: false,
    server: {
        port: 1420,
        strictPort: true,
        host: '127.0.0.1',
        watch: {
            // 不监听：tsconfig（vite macOS 误触发）、元数据文件（避免改版本号/配置就 reload）、vite 自己
            ignored: [
                '**/tsconfig*.json',
                '**/tsconfig*.tsbuildinfo',
                '**/package.json',
                '**/package-lock.json',
                '**/Cargo.toml',
                '**/Cargo.lock',
                '**/tauri.conf.json',
                '**/vite.config.*',
                '**/target/**',
                '**/dist/**',
                '**/node_modules/**',
            ],
            // 用轮询（每 300ms 检查一次），避开 macOS 上 fs.watch 对 vite.config.js 的误报
            usePolling: true,
            interval: 300,
        },
    },
    envPrefix: ['VITE_', 'TAURI_'],
    build: {
        target: 'es2021',
        minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
        sourcemap: !!process.env.TAURI_DEBUG,
    },
});
