import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  // Reserved project preview ports — VS Code Ports panel + Simple Browser
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    // Keep HMR stable inside VS Code / Cursor Simple Browser
    hmr: {
      host: "127.0.0.1",
      port: 5173,
      clientPort: 5173,
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
  },
});
