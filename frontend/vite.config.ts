import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The dev server proxies to the Rust server so the UI runs against a real
// backend without a second origin and without loosening CORS.
export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", sourcemap: false },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.HIVEMIND_DEV_TARGET ?? "http://127.0.0.1:8750",
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
