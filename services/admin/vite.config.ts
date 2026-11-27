import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

// 开发期把 /api 代理到本机服务端。服务端目前没有 CORS，代理是最省事的联调方式。
// 生产部署时把 `dist/` 交给同域网关托管，或通过 VITE_API_BASE 指向完整后端地址。
export default defineConfig({
  plugins: [vue()],
  server: {
    host: "127.0.0.1",
    port: 5180,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
