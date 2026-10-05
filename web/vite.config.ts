import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  plugins: [react()],
  build: { assetsDir: "ui-assets" },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.MONITOR_HUB || "http://127.0.0.1:28080",
        ws: true,
      },
      "/health": "http://127.0.0.1:28080",
    },
  },
});
