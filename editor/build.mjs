import * as esbuild from "esbuild";

// trunk sets TRUNK_PROFILE for its hooks.
const release = process.env.TRUNK_PROFILE === "release" || process.env.NODE_ENV === "production";

await esbuild.build({
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
