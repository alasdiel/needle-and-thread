import * as esbuild from "esbuild";
import { fileURLToPath } from "node:url";

// trunk sets TRUNK_PROFILE for its hooks.
const release = process.env.TRUNK_PROFILE === "release" || process.env.NODE_ENV === "production";

await esbuild.build({
  // Paths below are from this folder, wherever it's run from: trunk runs it from crates/desktop.
  absWorkingDir: fileURLToPath(new URL(".", import.meta.url)),
  entryPoints: { "needle-editor": "src/index.ts" },
  outdir: "dist",
  bundle: true,
  format: "iife",
  globalName: "NeedleEditor",
  target: "es2022",
  sourcemap: !release,
  minify: release,
  logLevel: "warning",
});
