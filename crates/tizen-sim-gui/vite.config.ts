import { readFileSync } from "node:fs";
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The installed version the GUI shows. release-please bumps it with the crates' (release-please-config.json).
const tauriConf = JSON.parse(readFileSync(path.resolve(__dirname, "tauri.conf.json"), "utf8"));

// The Tauri app loads the dev server at this fixed port (tauri.conf.json).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: { __TIZEN_SIM_VERSION__: JSON.stringify(tauriConf.version) },
  resolve: { alias: { "@": path.resolve(__dirname, "ui") } },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src/**", "**/target/**"] } },
  build: { outDir: "dist", target: "es2022" },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["ui/test/setup.ts"],
  },
});
