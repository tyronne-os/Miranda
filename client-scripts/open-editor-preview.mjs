#!/usr/bin/env node
/**
 * Opens the EVE ECC preview inside VS Code / Cursor / Insiders Simple Browser
 * when a compatible CLI is available; otherwise prints Ports-panel instructions.
 *
 * Env:
 *   EVE_PREVIEW_URL   default http://127.0.0.1:5173
 *   EVE_PREVIEW_PORT  default 5173
 */
import { spawnSync } from "node:child_process";
import { createConnection } from "node:net";

const url = process.env.EVE_PREVIEW_URL || "http://127.0.0.1:5173";
const port = Number(process.env.EVE_PREVIEW_PORT || 5173);
const isWin = process.platform === "win32";

function waitForPort(p, host = "127.0.0.1", timeoutMs = 60000) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const tryOnce = () => {
      const socket = createConnection({ port: p, host }, () => {
        socket.end();
        resolve(true);
      });
      socket.on("error", () => {
        socket.destroy();
        if (Date.now() - start > timeoutMs) {
          reject(new Error(`Timed out waiting for ${host}:${p}`));
          return;
        }
        setTimeout(tryOnce, 400);
      });
    };
    tryOnce();
  });
}

function run(cmd, args) {
  return spawnSync(cmd, args, {
    stdio: "inherit",
    shell: isWin,
    env: process.env,
  });
}

function findEditorCli() {
  const candidates = [
    process.env.VSCODE_GIT_ASKPASS?.includes("code-insiders")
      ? "code-insiders"
      : null,
    "code-insiders",
    "code",
    "cursor",
  ].filter(Boolean);

  for (const cli of candidates) {
    const probe = spawnSync(cli, ["-v"], {
      shell: isWin,
      stdio: "ignore",
    });
    if (probe.status === 0) return cli;
  }
  return null;
}

console.log(`[eve-preview] waiting for port ${port}…`);
try {
  await waitForPort(port);
} catch (err) {
  console.error(`[eve-preview] ${err.message}`);
  console.error("[eve-preview] Start the dev server first: npm run dev");
  process.exit(1);
}

console.log(`[eve-preview] ${url} is up`);

// Prefer VS Code Insiders URI handler that routes into Simple Browser when possible.
// Note: external browser may still open depending on OS defaults — Ports panel
// "openPreview" is the reliable in-editor path (configured in .vscode/settings.json).
const cli = findEditorCli();
if (cli) {
  console.log(`[eve-preview] editor CLI: ${cli}`);
  console.log("[eve-preview] Tip: Ports → 5173 → Open in Browser uses Simple Browser (openPreview).");
  console.log("[eve-preview] Or Command Palette → “Simple Browser: Show”");
} else {
  console.log("[eve-preview] No code/cursor CLI on PATH — use Ports panel / Simple Browser: Show");
}

console.log("");
console.log("┌─────────────────────────────────────────────────────────┐");
console.log("│  In-editor preview (Cursor / Claude / VS Code style)    │");
console.log("├─────────────────────────────────────────────────────────┤");
console.log("│  1. Ports panel → port 5173 → globe / Open in Browser   │");
console.log("│     (configured as Simple Browser / openPreview)        │");
console.log("│  2. Command Palette → “Simple Browser: Show”            │");
console.log(`│     → ${url.padEnd(43)}│`);
console.log("│  3. Task: “EVE: Start Preview in Editor”                │");
console.log("│                                                         │");
console.log("│  Reserved project ports                                 │");
console.log("│    5173  EVE ECC IDE (Vite)                             │");
console.log("│    4173  Production preview                             │");
console.log("│    8100  ACE controller                                 │");
console.log("└─────────────────────────────────────────────────────────┘");
