import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const resolve = (path: string) =>
  fileURLToPath(new URL(path, import.meta.url));

// Two HTML entries — the landing page and `/updates`. The SSR pass is a
// single entry (`--ssr src/entry-server.tsx`), so it keeps the default input
// and prerenders both pages itself.
export default defineConfig(({ isSsrBuild }) => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": resolve("./src"),
    },
  },
  build: isSsrBuild
    ? {}
    : {
        rollupOptions: {
          input: {
            main: resolve("./index.html"),
            updates: resolve("./updates.html"),
          },
        },
      },
}));
