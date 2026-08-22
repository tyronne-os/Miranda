#!/usr/bin/env node
/**
 * Fresh-machine bootstrap for EVE ECC (Windows-friendly).
 * Run from repo root: npm run setup
 */
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const isWin = process.platform === "win32";

function log(msg) {
  console.log(`▸ ${msg}`);
}

function run(cmd, args, cwd = root) {
  log(`${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, {
    cwd,
    stdio: "inherit",
    shell: isWin,
    env: process.env,
  });
  if (r.status !== 0) {
    process.exit(r.status ?? 1);
  }
}

log("EVE ECC setup — Instant Presence monorepo");
log(`root: ${root}`);
log(`node: ${process.version}`);

const envPath = join(root, ".env");
const envExample = join(root, ".env.example");
if (!existsSync(envPath) && existsSync(envExample)) {
  copyFileSync(envExample, envPath);
  log("created .env from .env.example");
} else {
  log(".env ready");
}

// Ensure workspace package folders exist for npm workspaces
for (const dir of ["services/ace-controller", "infra", "apps/web/public/staff"]) {
  mkdirSync(join(root, dir), { recursive: true });
}

run(isWin ? "npm.cmd" : "npm", ["install"]);

// Python controller deps (optional if python missing)
const py = spawnSync(isWin ? "py" : "python3", ["--version"], { shell: isWin });
const pyCmd = py.status === 0 ? (isWin ? "py" : "python3") : isWin ? "python" : "python3";
const pyCheck = spawnSync(pyCmd, ["--version"], { shell: isWin, encoding: "utf8" });
if (pyCheck.status === 0) {
  log(`python: ${(pyCheck.stdout || pyCheck.stderr || "").trim()}`);
  const req = join(root, "services/ace-controller/requirements.txt");
  if (existsSync(req)) {
    run(pyCmd, ["-m", "pip", "install", "-r", "requirements.txt"], join(root, "services/ace-controller"));
  }
} else {
  log("python not found — skip ace-controller deps (web IDE still works)");
}

const marker = join(root, ".eve-ecc-setup-ok");
writeFileSync(
  marker,
  `ok ${new Date().toISOString()}\nnode ${process.version}\n`,
  "utf8",
);

log("setup complete");
log("next:  npm run dev          → IDE  http://127.0.0.1:5173");
log("       npm run controller   → ACE  http://127.0.0.1:8100");
