import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 的 WebView 通过固定端口加载前端，因此 dev 端口必须固定
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // 与 Tauri 使用的 WebView2 对齐，避免产出多余降级代码
    target: "chrome110",
    sourcemap: false,
  },
});
