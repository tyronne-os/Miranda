#!/usr/bin/env node
// CAT-5 Model Routing Protocol — backlog scanner.
// Scans every .kiro/specs/*/tasks.md for [CAT n] tagged checkboxes and
// reports what's still open, broken out by CAT tier, so you know BEFORE
// starting a session whether you'll need the Opus 5 model-dropdown switch
// at all. See .kiro/steering/model-routing-protocol.md for the full rule.

import { readdirSync, readFileSync, statSync } from 'fs';
import { join } from 'path';

const SPECS_DIR = join(import.meta.dirname, '..', '.kiro', 'specs');

const MODEL_BY_CAT = {
  1: 'Qwen3 Coder Next',
  2: 'Amazon Nova Lite',
  3: 'Amazon Nova Pro',
  4: 'Claude Sonnet 5 (escalate to Opus 5 after 2 failed verifications)',
  5: 'Claude Opus 5 — MANDATORY, no exceptions',
};

function findTasksFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      const tasksPath = join(full, 'tasks.md');
      try {
        statSync(tasksPath);
        out.push({ workOrder: entry, path: tasksPath });
      } catch {
        // no tasks.md in this spec dir — skip
      }
    }
  }
  return out;
}

function parseTasks(path) {
  const lines = readFileSync(path, 'utf8').split('\n');
  const tasks = [];
  for (const line of lines) {
    const match = line.match(/^- \[( |x)\] \[CAT (\d)\] (.+)$/);
    if (!match) continue;
    tasks.push({ done: match[1] === 'x', cat: Number(match[2]), text: match[3] });
  }
  return tasks;
}

const results = findTasksFiles(SPECS_DIR).map((f) => ({
  workOrder: f.workOrder,
  tasks: parseTasks(f.path),
}));

const byCat = { 1: [], 2: [], 3: [], 4: [], 5: [] };
for (const { workOrder, tasks } of results) {
  for (const t of tasks) {
    if (!t.done) byCat[t.cat].push({ workOrder, text: t.text });
  }
}

console.log('=== CAT-5 Model Routing — pending task backlog ===\n');
for (const cat of [5, 4, 3, 2, 1]) {
  const pending = byCat[cat];
  console.log(`CAT ${cat} (${MODEL_BY_CAT[cat]}) — ${pending.length} pending`);
  for (const p of pending) {
    console.log(`  [${p.workOrder}] ${p.text.slice(0, 90)}${p.text.length > 90 ? '…' : ''}`);
  }
  console.log('');
}

const cat5Count = byCat[5].length;
if (cat5Count > 0) {
  console.log(`⚠ ${cat5Count} CAT 5 task(s) pending — you WILL need to switch to Opus 5 for these before the build is complete.`);
} else {
  console.log('✓ No CAT 5 tasks pending — Sonnet 5 / Haiku 4.5 can carry this session without an Opus switch.');
}
