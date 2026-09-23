import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../../..");
const target = process.env.CARGO_TARGET_DIR || resolve(repo, "target");
const profile = process.env.CONGMIAO_PROFILE || "release";
const args = process.argv.slice(2);
let requested = process.env.TAURI_ENV_TARGET_TRIPLE || "";
for (let index = 0; index < args.length; index += 1) {
  if (args[index] === "--target" && args[index + 1]) requested = args[index + 1];
}
const host = execSync("rustc -vV", { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"))
  .split(":")[1]
  .trim();
const triple = requested || host;
const build = ["cargo", "build", "-p", "congmiao-daemon", "--release"];
if (triple !== host) build.push("--target", triple);
execSync(build.join(" "), { cwd: repo, stdio: "inherit" });
const dest = resolve(here, "../src-tauri/binaries");
mkdirSync(dest, { recursive: true });
const suffix = triple.includes("windows") ? ".exe" : "";
const fromDir = triple === host ? resolve(target, profile) : resolve(target, triple, profile);
for (const name of ["congmiao-daemon", "congmiao-host"]) {
  const from = resolve(fromDir, `${name}${suffix}`);
  const to = resolve(dest, `${name}-${triple}${suffix}`);
  copyFileSync(from, to);
  if (!suffix) chmodSync(to, 0o755);
  console.log(to);
}
