import * as esbuild from "esbuild";

await esbuild.build({
  entryPoints: { "needle-editor": "src/index.ts" },
  outdir: "dist",
  bundle: true,
  format: "iife",
  globalName: "NeedleEditor",
  target: "es2022",
  sourcemap: true,
  minify: process.env.NODE_ENV === "production",
  logLevel: "warning",
});
