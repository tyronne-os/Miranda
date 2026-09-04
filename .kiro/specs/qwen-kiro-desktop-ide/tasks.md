# Implementation Plan: Qwen Kiro Desktop IDE

## Overview

This task plan implements a dual-mode desktop IDE combining Kiro's spec-driven workflow with Claude Desktop-style chat. The architecture spans 8 coordinated work orders: Tauri scaffold, FastAPI backend, React components, file watching, git integration, setup wizard, desktop integration, and testing/deployment. Each task is verified with real command output and measured against build standards (no simulated inference, all file operations validated, latency measured <100ms).

---

## Work Order 1: Tauri Scaffold and Project Structure

**CAT Assignment**: CAT 1 (mechanical setup), CAT 2 (window management)

**Purpose**: Foundation—establish Tauri project, directory structure, IPC bridge to FastAPI, and window lifecycle management.

- [ ] 1.1 Initialize Tauri project and workspace structure [CAT 1]
  - Run `cargo tauri init` with project name `qwen-kiro-ide`
  - Verify directory structure: `src-tauri/`, `src/`, `Cargo.toml`, `src-tauri/tauri.conf.json`
  - Create subdirectories: `backend/` (Python FastAPI), `src/` (React), `src-tauri/` (Rust)
  - Verify with: `find . -maxdepth 2 -type d | head -20`
  - _Requirements: 12.1, 13.1_

- [ ] 1.2 Define Tauri IPC command bridge for React-to-Rust calls [CAT 1]
  - Create `src-tauri/src/main.rs` with IPC command handlers: `invoke_read_file()`, `invoke_write_file()`, `invoke_list_files()`, `invoke_exec_command()`
  - Each handler forwards request to FastAPI HTTP endpoint (e.g., `invoke_read_file` → `POST http://localhost:8002/api/files/read`)
  - Implement error handling: catch HTTP errors, return JSON to React with error field
  - Verify: `cargo build` succeeds, IPC commands are registered in Tauri
  - _Requirements: 13.1, 13.4_

- [ ] 1.3 Implement window lifecycle and state persistence [CAT 2]
  - Create WindowManager module in `src-tauri/src/window.rs`
  - Persist window state (size, position, maximized flag) to `~/.config/qwen-kiro/window-state.json` on close
  - On startup, restore window state from file (or use defaults if first run)
  - Implement menu bar with File, Edit, View, Help menus
  - Verify with: `cargo build && cargo tauri build` — check that window opens and position persists across restarts
  - _Requirements: 12.2, 20.2_

- [ ] 1.4 Integrate Tauri file system API with path whitelist validation [CAT 1]
  - Add path validation utility: `validate_path(requested_path) -> Result<String, String>`
  - Whitelist roots: `/mnt/nobility-vault/`, `/home/hunt/`
  - Reject any path outside whitelist with error message "Path not in allowed roots"
  - Verify with: `cargo test validate_path` — test paths inside and outside whitelist
  - _Requirements: 13.5, 9.1_

- [ ] 1.5 Implement ProcessManager to spawn and monitor FastAPI backend [CAT 2]
  - Create `src-tauri/src/process.rs` with ProcessManager struct
  - On app startup, spawn FastAPI subprocess: `python backend/server.py`
  - Monitor process health; if FastAPI crashes, restart within 5 seconds
  - Log process output to `~/.config/qwen-kiro/backend.log`
  - Verify with: start app, kill FastAPI process, observe restart within 5s; check logs with `tail -f ~/.config/qwen-kiro/backend.log`
  - _Requirements: 13.6_

- [ ] 1.6 Create app configuration file and config loader [CAT 1]
  - Create `~/.config/qwen-kiro/config.json` on first run (touched by setup wizard task 6.1)
  - Config schema: `{project_vault, qwen14b_endpoint, qwencoder_endpoint, theme, recent_projects}`
  - Implement ConfigLoader in Rust: `load_config() -> AppConfig` and `save_config(cfg: AppConfig)`
  - Verify with: `cat ~/.config/qwen-kiro/config.json | jq .` — check valid JSON structure
  - _Requirements: 18.2, 20.2_

- [ ] 1.7 Checkpoint - Tauri foundation build succeeds [CAT 1]
  - Run `cargo build` from `src-tauri/` directory — verify no errors
  - Run `cargo tauri build` — verify AppImage or binary is created in target/release/
  - Test: launch built app, verify window opens and shows React frontend (stub component OK)
  - Verify with: `ls -la src-tauri/target/release/` and manual app launch
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 2: FastAPI Backend and Model Routing

**CAT Assignment**: CAT 2 (documented API), CAT 3 (model routing logic)

**Purpose**: Business logic layer—file I/O, git operations, model inference routing, terminal execution.

- [ ] 2.1 Initialize FastAPI project and server scaffold [CAT 1]
  - Create `backend/server.py` with FastAPI app, CORS enabled for localhost:3000 (React dev) and localhost (Tauri)
  - Add health check endpoint: `GET /api/health` — returns `{status: "ok", models: {qwen14b: bool, qwencoder: bool}}`
  - Create project structure: `backend/routes/`, `backend/models/`, `backend/utils/`
  - Verify with: `python -m pytest backend/test_server.py::test_health` or `curl http://localhost:8002/api/health`
  - _Requirements: 13.1, 13.2_

- [ ] 2.2 Implement file operation endpoints (create, read, write, delete) [CAT 2]
  - Create `backend/routes/files.py` with endpoints:
    - `GET /api/files/read?path={path}` — read file, return `{path, content, size_bytes, last_modified}`
    - `POST /api/files/write` — write file, body: `{path, content}`
    - `POST /api/files/create` — create file/folder, body: `{path, is_directory}`
    - `DELETE /api/files/delete?path={path}` — delete file
    - `GET /api/files/tree?project={name}` — return full directory tree as JSON
  - All endpoints call `validate_path()` before filesystem access
  - Return 400 error if path is outside whitelist
  - Verify with: `curl http://localhost:8002/api/files/read?path=/mnt/nobility-vault/projects/test/README.md` and `ls -la /mnt/nobility-vault/projects/test/README.md`
  - _Requirements: 13.2, 9.1-9.5_

- [ ] 2.3 Implement git operation endpoints [CAT 3]
  - Create `backend/routes/git.py` with endpoints:
    - `POST /api/git/init` — run `git init` in project directory
    - `POST /api/git/commit` — body: `{project, message}`, run `git commit -m {message}`
    - `POST /api/git/add-remote` — body: `{project, remote_url}`, run `git remote add origin {url}`
    - `POST /api/git/push` — body: `{project, remote, branch}`, run `git push {remote} {branch}`, queue if network fails
    - `GET /api/git/status?project={name}` — return `{modified_files, pending_commits, last_commit_hash}`
    - `GET /api/git/history?project={name}` — return last 10 commits with hash, message, timestamp
  - All git commands use subprocess.run() with shell=True (careful: sanitize message input)
  - Verify with: `cd /mnt/nobility-vault/projects/test && git log --oneline` and check output from endpoint
  - _Requirements: 10.1-10.5, 11.3-11.5_

- [ ] 2.4 Implement model routing endpoints for Qwen 14B and Coder [CAT 3]
  - Create `backend/routes/models.py` with endpoints:
    - `POST /api/qwen/14b` — body: `{messages, context_files}`, route to localhost:8000/v1/chat/completions
    - `POST /api/qwen/coder` — body: `{messages, context_files}`, route to localhost:8001/v1/chat/completions
  - Resolve file references: for each file in `context_files`, read file content and prepend to system prompt
  - Stream response back as Server-Sent Events (SSE) or WebSocket
  - Verify with: `curl -X POST http://localhost:8002/api/qwen/14b -H "Content-Type: application/json" -d '{"messages": [{"role": "user", "content": "Hello"}]}'` (real Qwen model must be running at localhost:8000)
  - _Requirements: 1.3, 1.4, 3.2-3.4_

- [ ] 2.5 Implement terminal/shell execution endpoint [CAT 2]
  - Create `POST /api/exec` — body: `{command, cwd}`, execute shell command via subprocess
  - Return `{stdout, stderr, exit_code, duration_ms}`
  - Validate `cwd` is within project directory or `/home/hunt/`
  - Capture output in real-time and stream to client via WebSocket or chunked response
  - Verify with: `curl -X POST http://localhost:8002/api/exec -d '{"command": "ls -la", "cwd": "/mnt/nobility-vault/projects/test"}'`
  - _Requirements: 4.2-4.5_

- [ ] 2.6 Implement spec workflow endpoints (create, list, generate tasks/code) [CAT 3]
  - Create `backend/routes/specs.py` with endpoints:
    - `POST /api/specs/create` — body: `{project, feature_name}`, mkdir `.kiro/specs/{feature_name}/`, create empty requirements.md
    - `GET /api/specs/list?project={name}` — list all spec directories in `.kiro/specs/`
    - `POST /api/specs/generate-tasks` — body: `{project, feature_name, spec_text}`, call Qwen 14B to generate tasks.md, write file, create git commit
    - `POST /api/specs/generate-code` — body: `{project, feature_name, task_id, task_text}`, call Qwen Coder, generate code file, create git commit
  - Each generate endpoint should call the model routing endpoints (2.4)
  - Verify with: `curl http://localhost:8002/api/specs/list?project=myproject` and `ls -la /mnt/nobility-vault/projects/myproject/.kiro/specs/`
  - _Requirements: 8.2-8.4_

- [ ] 2.7 Checkpoint - FastAPI build and health check passes [CAT 1]
  - Run `python -m pytest backend/test_*.py` or equivalent — all tests pass
  - Run `python backend/server.py` and verify no startup errors
  - Verify with: `curl http://localhost:8002/api/health` returns `{"status": "ok"}`
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 3: React Components and UI Layout

**CAT Assignment**: CAT 1 (component structure), CAT 2 (state management)

**Purpose**: Frontend—chat panel, file explorer, spec editor, mode toggle, keyboard shortcuts.

- [ ] 3.1 Initialize React project with Vite and TypeScript [CAT 1]
  - Run `npm create vite@latest . -- --template react-ts`
  - Install dependencies: `@tauri-apps/api`, `axios`, `react-router-dom`, `react-markdown`, `prism-react-renderer`
  - Verify with: `npm run dev` — React dev server starts on localhost:5173
  - _Requirements: 1.2, 6.1_

- [ ] 3.2 Create App context and state management (AppContext, useApp hook) [CAT 2]
  - Create `src/context/AppContext.tsx` with state: `{currentProject, planItMode, messageHistory, fileTree, selectedModel, gitStatus, modelStatus}`
  - Create `useApp()` hook for consuming context
  - Implement `ProjectProvider` wrapper
  - Verify with: `npm run build` — no TypeScript errors in context setup
  - _Requirements: 6.1, 6.2_

- [ ] 3.3 Implement Header component with mode toggle and model selector [CAT 1]
  - Create `src/components/Header.tsx` with:
    - ProjectSelector dropdown (recent projects list)
    - ModeToggle button (PLAN IT / ENGINEER IT, Alt+1/2 keyboard shortcuts)
    - ModelSelector dropdown (Qwen 14B / Qwen Coder)
    - StatusBar showing git status, sync status, model health
  - Toggle updates `planItMode` in context
  - Verify with: `npm run dev` and click mode toggle — see layout switch without page reload
  - _Requirements: 6.3, 2.1_

- [ ] 3.4 Implement ChatPanel component for PLAN IT mode [CAT 2]
  - Create `src/components/ChatPanel.tsx` with:
    - MessageHistory list (scrollable, auto-scroll on new message)
    - ChatInput textarea (multi-line, Shift+Enter for newline, Enter to send)
    - File reference autocomplete (@prefix → recent files)
    - Markdown rendering for assistant responses
    - Syntax highlighting for code blocks via Prism
  - Connect to context: read messageHistory, append on send
  - Verify with: send message "Hello", see it appear in history with markdown rendering
  - _Requirements: 1.1-1.7, 3.1-3.4_

- [ ] 3.5 Implement ProjectExplorer (FileTree) component for ENGINEER IT mode [CAT 2]
  - Create `src/components/ProjectExplorer.tsx` with:
    - Hierarchical file tree (folders collapsible/expandable)
    - File icons based on extension (folder, file, .py, .tsx, etc.)
    - Right-click context menu (Create File, Create Folder, Delete, Rename)
    - Breadcrumb path showing current directory
  - Connect to context: read fileTree, trigger API calls on file operations
  - Verify with: `npm run dev` and switch to ENGINEER IT mode — see file tree populate from `/mnt/nobility-vault/projects/{project}/`
  - _Requirements: 7.1-7.6_

- [ ] 3.6 Implement SpecPanel component (markdown editor + task list) [CAT 2]
  - Create `src/components/SpecPanel.tsx` with tabs:
    - Requirements editor (read/write requirements.md)
    - Design editor (read/write design.md)
    - Tasks checklist (read-only task list from tasks.md, checkboxes for completion tracking)
    - Code preview (display generated code from tasks)
    - History pane (audit trail: spec generation timestamps, task count, code generation count)
  - Save on Ctrl+S, create git commit via FastAPI
  - Verify with: create spec, type requirements, save with Ctrl+S — see git commit in history
  - _Requirements: 8.1-8.4_

- [ ] 3.7 Implement TerminalPane component [CAT 1]
  - Create `src/components/TerminalPane.tsx` with:
    - Output display (captures stdout/stderr from executed commands)
    - "Run" button on ```bash and ```python code blocks in chat
    - Confirmation dialog before execution
    - Manual command input (type command + Enter)
    - Up/down arrow history navigation
  - On Run: POST /api/exec, stream output to pane
  - Verify with: send message with ```bash code block, click Run, see confirmation dialog and output
  - _Requirements: 4.1-4.6_

- [ ] 3.8 Implement mode toggle and keyboard shortcut handler [CAT 1]
  - Create `src/hooks/useKeyboardShortcuts.ts` with handlers:
    - Alt+1: switch to PLAN IT mode
    - Alt+2: switch to ENGINEER IT mode
    - Ctrl+N: new file
    - Ctrl+Shift+N: new folder
    - Ctrl+S: save active file
    - Ctrl+B: toggle file tree visibility
    - Ctrl+`: toggle terminal pane
    - Ctrl+P: quick file search (fuzzy search overlay)
    - Ctrl+Tab/Shift+Tab: switch between open file tabs
  - Verify with: press Alt+1 and Alt+2 — mode switches without reloading
  - _Requirements: 19.1-19.3_

- [ ] 3.9 Implement SetupWizard component (first-run config) [CAT 1]
  - Create `src/components/SetupWizard.tsx` (modal overlay, 5 steps):
    - Step 1: Select default project vault (text input, default: /mnt/nobility-vault/projects/)
    - Step 2: Configure Qwen 14B endpoint (text input, default: localhost:8000/v1)
    - Step 3: Configure Qwen Coder endpoint (text input, default: localhost:8001/v1)
    - Step 4: Test connection button (ping /api/health, display Connected or Failed)
    - Step 5: Choose theme (light / dark radio buttons)
  - On completion, write config.json via FastAPI, save to context
  - Verify with: delete ~/.config/qwen-kiro/config.json, restart app — see setup wizard appear
  - _Requirements: 18.1-18.4_

- [ ] 3.10 Checkpoint - React build succeeds, all components render [CAT 1]
  - Run `npm run build` — verify no TypeScript errors
  - Run `npm run dev` — verify dev server starts
  - Verify with: `ls -la dist/` and browser showing React app with header, chat panel, file tree components
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 4: File Watcher and Real-Time Sync

**CAT Assignment**: CAT 3 (watchdog integration), CAT 4 (real-time sync correctness)

**Purpose**: Real-time file tree updates via WebSocket and watchdog library—verified <100ms latency.

- [ ] 4.1 Implement watchdog.Observer file system watcher in FastAPI [CAT 3]
  - Create `backend/watcher.py` with ProjectFileWatcher class (extends FileSystemEventHandler)
  - Implement handlers: `on_created()`, `on_modified()`, `on_deleted()`, `on_moved()`
  - Each handler broadcasts WebSocket event: `file_created {path}`, `file_modified {path}`, etc.
  - Start observer for project directory on project load
  - Verify with: create file in project directory while app is running, check FastAPI logs for watcher event
  - _Requirements: 7.4, 14.2-14.3_

- [ ] 4.2 Implement WebSocket connection and event broadcasting [CAT 3]
  - Create WebSocket endpoint in FastAPI: `ws://localhost:8002/ws/{project_name}`
  - On WebSocket connect, start file watcher for project directory
  - Broadcast file events from watcher to all connected clients
  - Handle WebSocket disconnect gracefully (cleanup observer)
  - Verify with: `npm run dev`, open browser DevTools → Network → WS tab, see WebSocket connect and events
  - _Requirements: 14.1, 14.2_

- [ ] 4.3 Implement React FileTree update handler for WebSocket events [CAT 2]
  - Create `src/hooks/useFileWatcher.ts` hook that connects to WebSocket
  - On file_created event: add node to fileTree state
  - On file_modified event: update node (re-read file content if open in editor)
  - On file_deleted event: remove node from fileTree
  - Verify file tree updates within 100ms via performance timing
  - Verify with: `npm run dev`, create file externally (`touch /mnt/nobility-vault/projects/test/new.txt`), see FileTree update instantly
  - _Requirements: 7.4, 14.3_

- [ ] 4.4 Implement WebSocket reconnection logic with polling fallback [CAT 3]
  - On WebSocket disconnect, retry connection every 2 seconds until successful
  - Fallback to HTTP polling if WebSocket fails 5 times in a row: `GET /api/files/tree?project={name}` every 5 seconds
  - Implement reconnection state in context: `websocketStatus: 'connected' | 'disconnecting' | 'polling'`
  - Display status indicator in header
  - Verify with: kill WebSocket connection (`npx kill-port 8002`), observe retry attempts and polling fallback in browser logs
  - _Requirements: 14.4-14.5_

- [ ] 4.5 Measure and verify file tree latency <100ms [CAT 4]
  - Create performance test: write file to disk, measure time until FileTree re-renders new node
  - Use Performance Observer API in browser or React Profiler
  - Log latency: "File tree update latency: XXms"
  - Verify with: `npm run dev`, create file, check browser DevTools Performance tab — measured latency <100ms
  - _Requirements: 7.4, 14.2_

- [ ] 4.6 Checkpoint - File watcher integration complete [CAT 3]
  - Run `python backend/test_watcher.py` — all watcher tests pass
  - Create file externally, verify FileTree updates within 100ms
  - Disconnect WebSocket (network cable or proxy), verify polling fallback activates
  - Verify with: manual test results and command output confirming each scenario
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 5: Git Integration and Auto-Commit

**CAT Assignment**: CAT 3 (git workflow), CAT 4 (atomic commits and push queuing)

**Purpose**: Automatic git commits on file save/spec create/code generate, with queued push on network failure.

- [ ] 5.1 Implement git auto-commit on file save [CAT 3]
  - Create `backend/git_manager.py` with GitManager class
  - On file write (from `/api/files/write`), call `GitManager.commit_file(project, file_path, message)`
  - Message format: `edit: Update {file_name}` (e.g., `edit: Update main.rs`)
  - Verify with: `curl http://localhost:8002/api/files/write` then `cd /mnt/nobility-vault/projects/test && git log --oneline | head -1` — see commit message
  - _Requirements: 10.1, 10.2_

- [ ] 5.2 Implement git auto-commit on spec creation and task generation [CAT 3]
  - When spec is created (POST /api/specs/create), auto-commit: `spec: Create {feature_name}`
  - When tasks are generated (POST /api/specs/generate-tasks), auto-commit: `gen: Generate tasks for {feature_name}`
  - When code is generated (POST /api/specs/generate-code), auto-commit: `gen: Implement {task_name}`
  - Verify with: create spec, generate tasks, inspect git log — see three new commits with correct messages
  - _Requirements: 10.1, 10.3-10.4_

- [ ] 5.3 Implement git push queue and retry logic [CAT 4]
  - Create `backend/push_queue.py` with PushQueue class
  - On commit, if remote URL is configured, queue async push task
  - If push succeeds: mark commit as synced
  - If push fails (network error): queue retry every 30 seconds, display "Sync pending" badge in header
  - If push fails (auth error): display "Sync failed - check credentials" banner, disable auto-push
  - Verify with: `curl http://localhost:8002/api/git/push` with network disconnected — see queue status, observe retry attempts
  - _Requirements: 11.3-11.5_

- [ ] 5.4 Implement git status endpoint with pending commits count [CAT 2]
  - `GET /api/git/status?project={name}` returns `{modified_files, pending_commits, pending_syncs, last_commit_hash}`
  - pending_commits = count of commits not yet synced to remote
  - pending_syncs = count of sync retries queued
  - Use this to populate StatusBar in React header
  - Verify with: `curl http://localhost:8002/api/git/status?project=test` — check JSON structure and values
  - _Requirements: 10.6, 11.1_

- [ ] 5.5 Implement git init on project creation [CAT 2]
  - When project is created (POST /api/projects), auto-run `git init` in project directory
  - Create initial commit: `init: Initialize {project_name}`
  - Verify with: `cd /mnt/nobility-vault/projects/newproject && git log --oneline` — see initial commit
  - _Requirements: 10.5_

- [ ] 5.6 Implement git remote URL handling (GitHub / Hugging Face) [CAT 3]
  - Create endpoint: `POST /api/git/add-remote` — body: `{project, remote_url}`
  - Validate URL format (github.com or huggingface.co)
  - Run `git remote add origin {url}`
  - On first save after remote configured, push queuing begins
  - Verify with: `curl http://localhost:8002/api/git/add-remote -d '{"project": "test", "remote_url": "..."}'` then `cd /mnt/nobility-vault/projects/test && git remote -v`
  - _Requirements: 11.1-11.2_

- [ ] 5.7 Verify git commit atomicity with concurrent file saves [CAT 4]
  - Create test: write 3 files in rapid succession (simulating concurrent saves)
  - Inspect git log: verify exactly 3 commits created, no partial commits
  - Verify atomicity: each file is committed only once, in isolation
  - Verify with: `python backend/test_git_atomicity.py` — test passes, confirms no race conditions
  - _Requirements: 10.1-10.4_

- [ ] 5.8 Checkpoint - Git workflow integration complete [CAT 3]
  - Run all git tests: `python -m pytest backend/test_git*.py`
  - Create spec, generate tasks, verify 3 new commits in `git log`
  - Configure remote URL, verify push begins and retries on network failure
  - Verify with: command output and git log showing all expected commits
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 6: Setup Wizard and Configuration

**CAT Assignment**: CAT 2 (UI wizard, config I/O)

**Purpose**: First-run setup—guide user through project vault, model endpoints, theme selection, and connection testing.

- [ ] 6.1 Implement SetupWizard detection (first-run check) [CAT 1]
  - On app startup, check if ~/.config/qwen-kiro/config.json exists
  - If not, display SetupWizard modal (block other UI)
  - If config exists, skip wizard and load config into context
  - Verify with: delete config, restart app — see wizard appear; complete wizard, restart app — wizard doesn't appear
  - _Requirements: 18.1-18.4_

- [ ] 6.2 Implement Step 1: Project vault directory selection [CAT 1]
  - SetupWizard Step 1: text input for project vault path, default `/mnt/nobility-vault/projects/`
  - Add "Browse" button to open file picker dialog (Tauri file dialog)
  - Verify selected path exists or offer to create it
  - Store in temporary wizard state
  - Verify with: enter path, click Browse, navigate to directory — path is set correctly
  - _Requirements: 18.1, 20.1_

- [ ] 6.3 Implement Step 2-3: Model endpoint configuration [CAT 1]
  - Step 2: text input for Qwen 14B endpoint (default: `localhost:8000/v1`)
  - Step 3: text input for Qwen Coder endpoint (default: `localhost:8001/v1`)
  - Store both in temporary state
  - Verify with: enter valid localhost URLs — proceed to next step
  - _Requirements: 18.1, 20.2_

- [ ] 6.4 Implement Step 4: Test connection button [CAT 2]
  - Add "Test Connection" button on Step 4
  - On click: POST /api/health (FastAPI checks both model endpoints)
  - If both models respond: display "Connected: Qwen 14B, Qwen Coder"
  - If one fails: display "Qwen 14B connected, Qwen Coder not available"
  - If both fail: display error, prevent proceeding to next step
  - Verify with: kill one model endpoint, click Test — see which is unavailable; restart model, click Test again — see connected
  - _Requirements: 18.1, 18.6_

- [ ] 6.5 Implement Step 5: Theme selection [CAT 1]
  - Step 5: radio buttons for Light / Dark theme (Dark is default)
  - Preview theme in real-time as user selects
  - Store selection in temporary state
  - Verify with: select Light theme — UI background changes to light gray; select Dark — background changes to dark
  - _Requirements: 16.1-16.3_

- [ ] 6.6 Implement wizard completion and config write [CAT 2]
  - On "Finish" button: validate all inputs
  - Write config.json to ~/.config/qwen-kiro/ via FastAPI: POST /api/config/write
  - Create ~/.config/qwen-kiro directory if not exists
  - Dismiss wizard, load config into context, render main app
  - Verify with: `cat ~/.config/qwen-kiro/config.json | jq .` — see all wizard inputs saved correctly
  - _Requirements: 18.2, 20.2_

- [ ] 6.7 Implement config reload and validation [CAT 1]
  - Create `src/hooks/useConfig.ts` hook that loads config.json on app startup
  - Validate config schema (all required fields present)
  - If config is corrupted: display warning, show wizard again
  - Verify with: manually corrupt config.json (remove a field), restart app — see warning and wizard
  - _Requirements: 18.3_

- [ ] 6.8 Checkpoint - Setup wizard fully functional [CAT 2]
  - Complete setup wizard from scratch: enter paths, test connections, select theme
  - Verify all inputs written to config.json
  - Delete config and restart: wizard appears again
  - Verify with: config file valid JSON, all fields present, app starts after completion
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 7: Desktop Integration and Deployment

**CAT Assignment**: CAT 1 (desktop file, installer), CAT 2 (AppImage/deb build)

**Purpose**: Install app to /mnt/nobility-vault/qwen-kiro-ide/, create .desktop file, register .qwen file association.

- [ ] 7.1 Create .desktop file for app menu integration [CAT 1]
  - Create `~/.local/share/applications/qwen-kiro.desktop` with:
    ```ini
    [Desktop Entry]
    Name=Qwen Kiro Desktop IDE
    Comment=Dual-mode AI-assisted development environment
    Exec=/mnt/nobility-vault/qwen-kiro-ide/qwen-kiro-ide
    Icon=editor
    Type=Application
    Categories=Development;IDE;
    MimeType=text/qwen;
    ```
  - Verify with: `desktop-file-validate ~/.local/share/applications/qwen-kiro.desktop` — must pass validation
  - _Requirements: 12.2, 12.3, 7.1_

- [ ] 7.2 Register .qwen file extension and MIME type [CAT 1]
  - Create `~/.local/share/mime/packages/qwen-project.xml` registering text/qwen MIME type
  - Associate text/qwen with qwen-kiro-ide application
  - Run `update-desktop-database` and `update-mime-database`
  - Verify with: `xdg-mime query filetype test.qwen` — should return `text/qwen`
  - _Requirements: 12.3_

- [ ] 7.3 Create installation script [CAT 1]
  - Create `scripts/install.sh` that:
    - Creates `/mnt/nobility-vault/qwen-kiro-ide/` directory
    - Copies built app binary to `/mnt/nobility-vault/qwen-kiro-ide/qwen-kiro-ide`
    - Copies FastAPI backend to `/mnt/nobility-vault/qwen-kiro-ide/backend/`
    - Creates Python venv and installs FastAPI, watchdog, httpx dependencies
    - Installs .desktop file
    - Registers .qwen MIME type
  - Script is idempotent: can run multiple times without errors
  - Verify with: `bash scripts/install.sh && ls -la /mnt/nobility-vault/qwen-kiro-ide/`
  - _Requirements: 12.1_

- [ ] 7.4 Build Tauri AppImage and .deb packages [CAT 2]
  - Run `cargo tauri build` from src-tauri/ — generates AppImage in `target/release/bundle/appimage/`
  - Run Tauri deb builder — generates .deb in `target/release/bundle/deb/`
  - Verify AppImage is executable and starts app: `./target/release/bundle/appimage/qwen-kiro-ide-*.AppImage`
  - Verify .deb can be installed: `sudo dpkg -i target/release/bundle/deb/qwen-kiro-ide_*.deb`
  - Verify with: `ls -la target/release/bundle/` and manual execution test
  - _Requirements: 12.1_

- [ ] 7.5 Create uninstall script [CAT 1]
  - Create `scripts/uninstall.sh` that:
    - Removes `/mnt/nobility-vault/qwen-kiro-ide/`
    - Removes `~/.local/share/applications/qwen-kiro.desktop`
    - Removes `~/.local/share/mime/packages/qwen-project.xml`
    - Runs `update-desktop-database` and `update-mime-database`
  - Verify with: run uninstall script, confirm app no longer appears in menu and .qwen files are unassociated
  - _Requirements: 12.1_

- [ ] 7.6 Checkpoint - Desktop integration complete [CAT 1]
  - Run install script: `bash scripts/install.sh`
  - Verify app appears in application menu
  - Verify .qwen files are associated with app (double-click opens in Qwen Kiro)
  - Verify with: `desktop-file-validate` passing, app menu showing entry, double-clicking .qwen file opens app
  - Ensure all tests pass, ask the user if questions arise.

---

## Work Order 8: Testing, Integration, and Deployment

**CAT Assignment**: CAT 2 (unit tests), CAT 3 (integration tests), CAT 4 (end-to-end)

**Purpose**: Comprehensive testing—path validation, message round-trip, file tree consistency, git atomicity, model failover, command execution safety.

- [ ] 8.1 Implement unit tests for path validation [CAT 2]
  - Create `backend/test_path_validation.py` with test cases:
    - `test_whitelist_allowed_paths()` — paths in `/mnt/nobility-vault/` and `/home/hunt/` are allowed
    - `test_whitelist_reject_paths()` — paths in `/etc/`, `/var/`, outside whitelist are rejected
    - `test_symlink_escape_attempt()` — symlink pointing outside whitelist is rejected
  - All tests must pass: `pytest backend/test_path_validation.py`
  - Verify with: `python -m pytest backend/test_path_validation.py -v`
  - _Requirements: 13.5, 9.1_

- [ ] 8.2 Implement unit tests for message history persistence [CAT 2]
  - Create `src/__tests__/MessageHistory.test.ts` (or Jest equivalent):
    - `test_message_round_trip()` — send message, write to .jsonl, load, verify content identical
    - `test_corrupted_jsonl_handling()` — if .jsonl has invalid JSON line, parser skips it and logs warning
    - `test_auto_scroll_on_load()` — when loading old messages, scroll position restored
  - All tests pass: `npm run test`
  - Verify with: `npm run test -- MessageHistory`
  - _Requirements: 5.1-5.2_

- [ ] 8.3 Implement integration test for file tree consistency [CAT 3]
  - Create `backend/test_file_tree_consistency.py`:
    - `test_file_created_updates_tree()` — create file, call `/api/files/tree`, verify new file appears
    - `test_file_deleted_updates_tree()` — delete file, call `/api/files/tree`, verify file removed
    - `test_nested_folder_structure()` — create nested folders, verify tree reflects hierarchy
  - Measure latency: time between file creation and tree API response < 50ms
  - All tests pass: `pytest backend/test_file_tree_consistency.py`
  - Verify with: measured latency log output: `File tree consistency latency: XXms`
  - _Requirements: 7.1-7.4, 4.2_

- [ ] 8.4 Implement integration test for git atomicity [CAT 3]
  - Create `backend/test_git_atomicity.py`:
    - `test_concurrent_file_saves()` — save 5 files rapidly, verify exactly 5 commits in git log (not merged, not partial)
    - `test_commit_message_format()` — verify all commits follow `{action}: {resource}` format (e.g., `edit: main.rs`)
    - `test_git_remote_push_queue()` — disable network, create commit, verify push queued; enable network, verify push succeeds
  - All tests pass: `pytest backend/test_git_atomicity.py`
  - Verify with: `git log --oneline` showing exact commit count, commit messages format validated
  - _Requirements: 10.1-10.4_

- [ ] 8.5 Implement integration test for model failover [CAT 3]
  - Create `backend/test_model_failover.py`:
    - `test_model_unavailable_at_startup()` — kill Qwen 14B, start app, verify health check detects unavailability, model selector disabled
    - `test_model_becomes_available()` — start with model down, start model, app detects within 5s, re-enables selector
    - `test_fallback_to_available_model()` — if Qwen 14B down but Qwen Coder up, chat requests route to Coder
  - Health check interval 5 seconds (per Req 2.4)
  - Verify with: measured timing: "Model failover detection: XXms" < 5 seconds
  - _Requirements: 2.3-2.5_

- [ ] 8.6 Implement integration test for command execution safety [CAT 3]
  - Create `src/__tests__/TerminalPane.test.ts`:
    - `test_confirmation_dialog_shown()` — send bash code block, verify "Run" button renders, clicking shows confirmation
    - `test_command_output_captured()` — execute `echo "test"`, verify output appears in TerminalPane
    - `test_exit_code_displayed()` — execute `exit 1`, verify exit code 1 shown in red
    - `test_command_history_navigation()` — type command, press up arrow, verify previous command recalled
  - Verify with: `npm run test -- TerminalPane`
  - _Requirements: 4.2-4.6_

- [ ] 8.7 Implement property-based tests for correctness properties [CAT 3]
  - Create property tests for design properties 1-12 (e.g., using hypothesis or fast-check):
    - Property 1: Path validation — generate random paths, verify whitelist check correct
    - Property 2: Message round-trip — generate random messages, verify serialization preserves content
    - Property 3: File tree consistency — random file ops, verify tree reflects within 100ms
    - Property 4: Git atomicity — concurrent saves, verify each creates exactly one commit
    - Property 5: Model failover — simulate random endpoint failures, verify detection <5s
  - Verify with: `pytest backend/test_properties.py` — all properties pass
  - _Requirements: All design correctness properties_

- [ ] 8.8 Implement end-to-end workflow test [CAT 4]
  - Create `backend/test_e2e_workflow.py`:
    - Start app (Tauri + FastAPI)
    - Create new project
    - Write spec via PLAN IT chat
    - Generate tasks via ENGINEER IT spec panel
    - Generate code from task
    - Execute generated code via terminal
    - Verify all files created, commits logged, UI reflects all changes
  - Run full workflow and capture output
  - Verify with: `python backend/test_e2e_workflow.py --headless` (or manually with screenshots)
  - _Requirements: 1-20 (full requirements coverage)_

- [ ] 8.9 Benchmark and measure latency [CAT 3]
  - Measure and log all critical latencies:
    - File tree update (creation to UI render): target <100ms
    - Message send-to-display: target <500ms (including model latency)
    - WebSocket reconnection: target <2s
    - Model failover detection: target <5s
    - Terminal command execution: target <1s (capture and display)
  - Create benchmark report: `BENCHMARKS.md` with measured values
  - Verify with: `cat BENCHMARKS.md | grep latency` showing all measurements
  - _Requirements: 7.4, 14.3, 4.5, 2.4_

- [ ] 8.10 Create deployment checklist and documentation [CAT 1]
  - Create `DEPLOYMENT.md` with:
    - Prerequisites (Python 3.11+, Node.js 18+, Git, Rust cargo)
    - Build instructions (npm build, cargo build)
    - Install instructions (run scripts/install.sh)
    - Configuration (first-run setup wizard)
    - Troubleshooting (model endpoint not responding, git errors)
    - Uninstall (scripts/uninstall.sh)
  - Verify with: follow DEPLOYMENT.md from scratch — app installs and runs successfully
  - _Requirements: 12.1-12.2_

- [ ] 8.11 Checkpoint - All tests pass, deployment ready [CAT 3]
  - Run all test suites: `npm run test && pytest backend/test_*.py`
  - Verify no errors or warnings
  - Run end-to-end workflow test (manual or automated)
  - Verify benchmarks meet targets
  - Verify with: command output showing all tests passing, benchmark report showing <100ms file tree latency
  - Ensure all tests pass, ask the user if questions arise.

---

## Notes

- **Testing philosophy**: All test tasks reference specific correctness properties from the design document. Tests are not optional—they verify that requirements are met in measurable, testable ways.
- **CAT tier enforcement**: This plan strictly follows the CAT-5 Model Routing Protocol. CAT 1-2 tasks run on Qwen3 Coder Next / Amazon Nova Lite. CAT 3 tasks run on Amazon Nova Pro. CAT 4 tasks start on Claude Sonnet 5; if they fail verification twice, escalate to Claude Opus 5 (WO-4 and WO-5's file tree latency + git atomicity are the likely candidates).
- **Verification without simulation**: Every task includes a "Verify with" command that produces real output. No task is considered complete until the verifying command runs and produces evidence (e.g., `ls -la`, `git log`, `curl`, test output, measured latency).
- **Dependencies across work orders**:
  - WO-1 (Tauri scaffold) must complete before WO-2-8 can begin (app foundation)
  - WO-2 (FastAPI) must complete before WO-3 (React) connects to backend
  - WO-3 (React) and WO-4 (file watcher) can run in parallel once WO-2 endpoints are defined
  - WO-5 (git) depends on WO-2 git endpoints
  - WO-6 (setup wizard) depends on WO-2 and WO-3 UI
  - WO-7 (deployment) depends on all prior work orders
  - WO-8 (tests) can begin once WO-1-6 are stable (tests verify integration)

---

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.3", "1.4"] },
    { "id": 1, "tasks": ["1.5", "1.6", "2.1", "2.2"] },
    { "id": 2, "tasks": ["2.3", "2.4", "2.5", "2.6"] },
    { "id": 3, "tasks": ["3.1", "3.2", "3.3", "3.4"] },
    { "id": 4, "tasks": ["3.5", "3.6", "3.7", "3.8"] },
    { "id": 5, "tasks": ["3.9", "4.1", "4.2", "4.3"] },
    { "id": 6, "tasks": ["4.4", "4.5", "5.1", "5.2"] },
    { "id": 7, "tasks": ["5.3", "5.4", "5.5", "5.6"] },
    { "id": 8, "tasks": ["5.7", "6.1", "6.2", "6.3"] },
    { "id": 9, "tasks": ["6.4", "6.5", "6.6", "6.7"] },
    { "id": 10, "tasks": ["7.1", "7.2", "7.3", "7.4"] },
    { "id": 11, "tasks": ["7.5", "8.1", "8.2", "8.3"] },
    { "id": 12, "tasks": ["8.4", "8.5", "8.6", "8.7"] },
    { "id": 13, "tasks": ["8.8", "8.9", "8.10"] }
  ]
}
```
