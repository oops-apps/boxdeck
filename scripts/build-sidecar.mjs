// Builds boxdeck-mcp and copies it to src-tauri/binaries/boxdeck-mcp-<target-triple>, where
// Tauri's `bundle.externalBin` expects it. Runs as part of `tauri dev` / `tauri build`.
//   node scripts/build-sidecar.mjs [--debug]
// The target comes from TAURI_ENV_TARGET_TRIPLE (set by the Tauri CLI) or the host triple.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const debug = process.argv.includes("--debug");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)/m)[1];
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const exe = triple.includes("windows") ? ".exe" : "";

const args = ["build", "-p", "boxdeck-mcp", "--target", triple];
if (!debug) args.push("--release");
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const built = join(targetDir, triple, debug ? "debug" : "release", `boxdeck-mcp${exe}`);
const dest = join(root, "src-tauri", "binaries", `boxdeck-mcp-${triple}${exe}`);
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(built, dest);
console.log(`sidecar: ${dest}`);
