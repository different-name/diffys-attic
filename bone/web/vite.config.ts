import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: {
    proxy: {
      "/api": {
        target: process.env.BONE_API ?? "http://127.0.0.1:8790",
        changeOrigin: true,
      },
    },
  },
});
