import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  build: {
    target: ["es2021", "chrome105", "safari14.1"],
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: true,
    restoreMocks: true,
  },
});
