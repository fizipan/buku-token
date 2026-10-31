import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "./src") } },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    globals: true,
    // Each test renders React with Recharts, Radix and userEvent in jsdom: 0.1-0.4 s on an idle Mac, but the
    // default 5 s limit trips when the machine is busy (a cargo build, or the whole suite in parallel). The
    // tests were not wrong, only slow, so the limit is raised (also see asyncUtilTimeout in test/setup.ts).
    testTimeout: 20_000,
    hookTimeout: 20_000,
  },
});
