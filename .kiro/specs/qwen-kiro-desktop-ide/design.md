# Design Document: Qwen Kiro Desktop IDE

## Architecture Overview

Qwen Kiro Desktop IDE is a dual-mode desktop application built on Tauri (lightweight, native, full file system access). The architecture separates concerns into three layers:

1. **Tauri Runtime Layer** (Rust): Filesystem operations, window management, system tray, file dialogs
2. **FastAPI Backend** (Python): HTTP/WebSocket server providing file I/O, git operations, model inference routing, and real-time file watching
3. **React Frontend** (TypeScript): UI components for chat, file explorer, spec editor, and terminal

**Design Rationale**: 
- Tauri provides lightweight desktop packaging without Electron overhead
- FastAPI enables clean separation between UI (React) and backend logic (file ops, git, model routing)
- WebSocket + watchdog library enables real-time file tree sync matching Kiro's IPC bus responsiveness
- Local-only models (Qwen 14B/Coder on localhost:8000/8001) ensure offline operation and cost predictability
- Three-tier architecture mirrors Miranda's node model: discrete services connected via well-defined contracts

---

## Layered Component Architecture

### Layer 1: Tauri Runtime (Rust Backend)

**Purpose**: OS-level interactions and process lifecycle management

**Components**:

1. **FileSystemBridge** (main.rs)
   - Validates all filesystem paths against whitelist: `/mnt/nobility-vault/`, `/home/hunt/`
   - Invokes FastAPI for actual file operations (not direct FS access)
   - Handles Tauri invoke commands from React: `invoke('read_file', {path})` → FastAPI `/api/files/read`
   - Manages application lifecycle: startup, config loading, window state persistence

2. **ProcessManager**
   - Spawns FastAPI subprocess on startup: `python backend/server.py`
   - Monitors FastAPI health; restarts on crash (within 5 seconds)
   - Spawns shell commands for `git` operations via subprocess (called from FastAPI)

3. **WindowManager**
   - Creates main window with React frontend
   - Manages window state: size, position, maximized flag (persisted to config.json)
   - Handles menu bar and desktop integration (.desktop file association)

---

### Layer 2: FastAPI Backend (Python)

**Purpose**: Business logic, filesystem I/O, git operations, model routing, real-time file watching

**Endpoints**:

#### File Operations
- `GET /api/projects` — List all projects in vault
- `POST /api/projects` — Create new project
- `GET /api/files/tree?project={name}` — Fetch full directory tree
- `GET /api/files/read?path={full_path}` — Read file content
- `POST /api/files/write` — Write file content (body: `{path, content}`)
- `POST /api/files/create` — Create new file/folder (body: `{path, is_directory}`)
- `DELETE /api/files/delete?path={full_path}` — Delete file
- `POST /api/files/rename` — Rename file (body: `{old_path, new_path}`)

#### Git Operations
- `POST /api/git/init` — Initialize git repo in project
- `POST /api/git/commit` — Create commit (body: `{project, message}`)
- `POST /api/git/push` — Push to remote (body: `{project, remote, branch}`)
- `POST /api/git/pull` — Pull from remote (body: `{project, remote, branch}`)
- `POST /api/git/add-remote` — Add remote URL (body: `{project, remote_url}`)
- `GET /api/git/status?project={name}` — Get git status (modified files, commits pending)
- `GET /api/git/history?project={name}` — Get commit history

#### Model Inference Routing
- `POST /api/qwen/14b` — Route message to Qwen 14B (body: `{messages, context_files}`)
- `POST /api/qwen/coder` — Route message to Qwen Coder (body: `{messages, context_files}`)
- `GET /api/health` — Check model endpoint availability (pings localhost:8000 and localhost:8001)

#### Spec Workflow
- `POST /api/specs/create` — Create new spec (body: `{project, feature_name, description}`)
- `POST /api/specs/generate-tasks` — Generate tasks.md from spec (body: `{project, feature_name}`)
- `POST /api/specs/generate-code` — Generate code for task (body: `{project, feature_name, task_id}`)
- `GET /api/specs/list?project={name}` — List all specs in project

#### Terminal Execution
- `POST /api/exec` — Execute shell command (body: `{command, cwd}`)

#### WebSocket Events (ws://:8002/ws/{project_name})
- `file_created {path}` — New file created
- `file_modified {path}` — File modified
- `file_deleted {path}` — File deleted
- `git_commit {hash, message}` — Commit created
- `model_status {model, available}` — Model endpoint status changed

**File Watching Implementation**:
```python
import watchdog
from watchdog.observers import Observer
from watchdog.events import FileSystemEventHandler

class ProjectFileWatcher(FileSystemEventHandler):
    def on_created(self, event):
        await broadcast_ws(f"file_created {event.src_path}")
    
    def on_modified(self, event):
        await broadcast_ws(f"file_modified {event.src_path}")
    
    def on_deleted(self, event):
        await broadcast_ws(f"file_deleted {event.src_path}")

# Start observer per project directory
observer = Observer()
observer.schedule(ProjectFileWatcher(), path=project_dir, recursive=True)
observer.start()
```

**Model Routing**:
```python
@app.post("/api/qwen/14b")
async def qwen_14b_chat(request: ChatRequest):
    # Resolve file references (@file1.txt) to file content
    context = resolve_file_references(request.context_files, request.project)
    
    # Send to localhost:8000/v1 (OpenAI-compatible API)
    response = await httpx.post(
        "http://localhost:8000/v1/chat/completions",
        json={
            "model": "qwen-14b",
            "messages": request.messages + [{"role": "system", "content": context}],
            "stream": True
        }
    )
    # Stream response back to client
    async for line in response.aiter_lines():
        yield parse_sse(line)
```

**Path Validation**:
```python
ALLOWED_ROOTS = ["/mnt/nobility-vault/", "/home/hunt/"]

def validate_path(requested_path: str) -> bool:
    real_path = os.path.realpath(requested_path)
    return any(real_path.startswith(root) for root in ALLOWED_ROOTS)
```

---

### Layer 3: React Frontend

**Purpose**: UI rendering, user interaction, state management

**Component Hierarchy**:

```
App.tsx (main, manages mode state: planIt: boolean)
├── Header
│   ├── ProjectSelector (dropdown: recent projects + browse)
│   ├── ModeToggle (Alt+1 / Alt+2, current mode highlighted)
│   ├── ModelSelector (Qwen 14B / Qwen Coder)
│   └── StatusBar (git status, sync status, model health)
├── ChatPanel (PLAN IT Mode, full-screen)
│   ├── MessageHistory (list of messages, auto-scroll on new)
│   ├── ChatInput (multi-line, Shift+Enter for newline, Enter to send)
│   ├── TerminalPane (collapsible, shows command output)
│   └── FileReferenceAutocomplete (@prefix → recent files)
├── ProjectPanel (ENGINEER IT Mode, split layout)
│   ├── ProjectExplorer (left, collapsible)
│   │   ├── FileTree (hierarchical, icons, chevron expand/collapse)
│   │   ├── ContextMenu (create, delete, rename, open in terminal)
│   │   └── BreadcrumbPath (current directory path)
│   └── SpecPanel (right, tab-based)
│       ├── SpecEditor (markdown editor for requirements/design/tasks)
│       ├── TasksList (checklist view of tasks.md)
│       ├── CodePreview (shows generated code, copy button)
│       └── HistoryPane (audit trail of spec generations)
├── SetupWizard (first-run, modal overlay)
│   ├── VaultPathSelector (default: /mnt/nobility-vault/projects/)
│   ├── ModelEndpointConfig (localhost:8000, localhost:8001)
│   ├── TestConnectionButton (pings /api/health)
│   └── ThemeSelector (light / dark)
└── KeyboardShortcutHandler
    ├── Global shortcuts (Alt+1/2, Ctrl+P, Ctrl+B, etc.)
    └── Context-aware shortcuts (Ctrl+S saves active file in ENGINEER IT)
```

**State Management** (React Context API):
```typescript
// AppContext.ts
interface AppState {
  currentProject: string;
  planItMode: boolean;  // false = ENGINEER IT
  messageHistory: Message[];
  fileTree: FileNode[];
  selectedModel: "qwen-14b" | "qwen-coder";
  config: AppConfig;
  gitStatus: GitStatus;
  modelStatus: { qwen14b: boolean; qwenCoder: boolean };
}

// Message type for both PLAN IT chat and command history
interface Message {
  id: string;
  timestamp: number;
  role: "user" | "assistant";
  content: string;
  fileReferences?: string[];
  commandOutput?: string;  // for terminal pane
}
```

**File Reference Parser**:
```typescript
// ChatPanel.tsx
function resolveFileReferences(message: string, project: string): FileReference[] {
  const pattern = /@([\w\-/.]+|"[^"]*")/g;
  const matches = message.matchAll(pattern);
  
  return Array.from(matches).map(match => {
    const path = match[1].replace(/^"|"$/g, '');
    return {
      raw: match[0],
      resolved: path.startsWith('/') 
        ? path 
        : `/mnt/nobility-vault/projects/${project}/${path}`,
      exists: checkPathExists(resolved)
    };
  });
}
```

**Real-Time File Tree Updates**:
```typescript
// ProjectExplorer.tsx
useEffect(() => {
  const ws = new WebSocket(`ws://localhost:8002/ws/${currentProject}`);
  
  ws.onmessage = (event) => {
    const [action, path] = event.data.split(' ');
    
    switch(action) {
      case 'file_created':
      case 'file_modified':
      case 'file_deleted':
        setFileTree(prev => updateTreeNode(prev, path, action));
        break;
    }
  };
  
  return () => ws.close();
}, [currentProject]);
```

---

## Data Flow Diagrams

### PLAN IT Mode: Chat and Command Execution

```
User types message in ChatInput
  ↓
FileReference parser (@file.txt) → resolve paths, read content
  ↓
Send to selected model via POST /api/qwen/{14b|coder}
  ↓
FastAPI resolves file refs, routes to localhost:8000 or 8001
  ↓
Model streaming response (SSE or WebSocket)
  ↓
React receives chunk, appends to messageHistory state
  ↓
MessageHistory re-renders, auto-scrolls to latest
  ↓
User sees response in real-time
  ↓
[Optional] User clicks "Run" button on ```bash code block
  ↓
ChatInput detects code block, renders Run button
  ↓
User confirms in dialog
  ↓
POST /api/exec {command, cwd}
  ↓
FastAPI: subprocess.run(command) → capture stdout/stderr
  ↓
Append result to TerminalPane
  ↓
Write to messages.jsonl: {role: "assistant", content: result}
```

### ENGINEER IT Mode: Spec Workflow

```
User clicks "New Spec" button
  ↓
Modal prompts for feature name (kebab-case)
  ↓
POST /api/specs/create {project, feature_name}
  ↓
FastAPI: mkdir /mnt/nobility-vault/projects/{project}/.kiro/specs/{feature_name}/
  ↓
SpecPanel opens with empty requirements.md editor
  ↓
User writes spec, clicks "Generate Tasks"
  ↓
Read spec content from editor
  ↓
POST /api/specs/generate-tasks {project, feature_name, spec_text}
  ↓
FastAPI: POST /api/qwen/14b with spec as context
  ↓
Qwen 14B generates tasks.md content
  ↓
FastAPI: write tasks.md, create git commit "gen: Generate tasks"
  ↓
WebSocket: file_created event → FileTree updates
  ↓
SpecPanel: TasksList tab shows parsed tasks (checkbox list)
  ↓
User selects task, clicks "Generate Code"
  ↓
POST /api/specs/generate-code {project, feature_name, task_id}
  ↓
FastAPI: POST /api/qwen/coder with task context
  ↓
Qwen Coder generates code
  ↓
FastAPI: write code file, create git commit "gen: Implement {task}"
  ↓
WebSocket: file_created → FileTree updates, CodePreview displays new file
  ↓
User can Run code via ChatPanel TerminalPane
```

### Real-Time File Sync

```
External process writes to /mnt/nobility-vault/projects/myproj/file.txt
  ↓
FastAPI watchdog detects file_modified event
  ↓
FastAPI broadcasts ws.send("file_modified /mnt/nobility-vault/projects/myproj/file.txt")
  ↓
React WebSocket onmessage handler receives event
  ↓
Call updateTreeNode(fileTree, path, "file_modified")
  ↓
FileTree state updates (triggers re-render)
  ↓
If user has editor open for that file, prompt: "File changed. Reload? Keep? Merge?"
```

### Git Auto-Commit Flow

```
User saves file (Ctrl+S in editor)
  ↓
ChatPanel detects Ctrl+S, calls POST /api/files/write
  ↓
FastAPI writes file
  ↓
FastAPI auto-detects file change (watchdog)
  ↓
FastAPI: git add {file}
  ↓
FastAPI: git commit -m "edit: Update {filename}"
  ↓
WebSocket broadcasts: git_commit {hash, message}
  ↓
StatusBar updates: "Last commit: edit: Update file.txt (2 minutes ago)"
  ↓
If remote URL configured:
  ↓
FastAPI queues push: git push origin main
  ↓
StatusBar: "Syncing... (pending: 1 commit)"
  ↓
On success: "Synced to origin/main"
  ↓
On failure: queue retry every 30 seconds (show warning badge)
```

---

## Key Design Decisions and Tradeoffs

### 1. **FastAPI vs Direct IPC Calls**
**Decision**: Use FastAPI HTTP/WebSocket instead of shared memory IPC
**Rationale**:
- Mirrors Kiro's design: discrete services with well-defined contracts
- Easier testing (HTTP requests vs IPC memory layout)
- Future-proof: can move FastAPI to remote machine without UI changes
- WebSocket for real-time updates matches Kiro's latency targets (<100ms file tree update)
- Simpler security model: HTTP path validation vs IPC boundary protection

### 2. **Local Models Only (No Remote API Routing)**
**Decision**: Qwen models are always localhost:8000/8001, no remote Bedrock/Anthropic fallback
**Rationale**:
- Requirement 15 specifies full offline operation
- Simpler endpoint routing (no model switching overhead)
- Future work can add remote routing in FastAPI layer without UI changes
- Matches "batteries included" spirit: inference is local, not cloud-dependent

### 3. **Three-Tier Separation**
**Decision**: Tauri (OS) → FastAPI (logic) → React (UI)
**Rationale**:
- Isolation: UI crashes don't affect backend; backend crashes trigger auto-restart
- Scalability: FastAPI can be distributed later if needed
- Testing: Each tier has clear input/output contracts
- Maintenance: Python for logic, TypeScript for UI (familiar tooling per domain)

### 4. **File Watching via Watchdog, Not Direct FS Polling**
**Decision**: FastAPI uses watchdog.Observer, not periodic filesystem scans
**Rationale**:
- Instant detection (<10ms) vs polling overhead (5-second interval Req 14)
- Lower CPU usage (event-driven vs timer-loop)
- Matches real-time requirement: "update within 100ms" (Req 7.4)
- Kiro's IPC bus principle: latency-sensitive operations need event-driven design

### 5. **Git Auto-Commit with Queued Push**
**Decision**: Commit immediately, push in background with retry queue
**Rationale**:
- Prevents UI blocking on network latency
- Guarantees work is saved locally first (no data loss if push fails)
- Matches GitHub Desktop model: local commits always persist, sync is best-effort
- Requirement 11.4: queue push on network failure

### 6. **WebSocket + Polling Fallback**
**Decision**: Primary WebSocket for file tree, fallback to HTTP polling if WS unavailable
**Rationale**:
- Requirement 14.5: polling fallback for robustness
- WebSocket for responsive UX in normal case
- Graceful degradation: 5-second file tree update latency is acceptable if WS fails
- Prevents "stuck" UI if firewall blocks WebSocket

### 7. **ENGINEER IT Mode Dominance (90% UX focus)**
**Decision**: Largest screen real estate for spec panel, chat relegated to secondary
**Rationale**:
- User's stated preference (from clarification question)
- Spec workflow is primary use case (spec → tasks → code)
- Chat (PLAN IT) available but not center stage
- Tab-based SpecPanel allows chat history to be visible in background

### 8. **Message History in JSONL, Not SQLite**
**Decision**: Append-only JSONL in .history/messages.jsonl per project
**Rationale**:
- Simple, human-readable format (easy manual inspection/edit if needed)
- Git-friendly (line-based diffs)
- No database schema to manage
- Matches Kiro's principle: spec artifacts as versioned files, not opaque storage

### 9. **Configuration in config.json, Not GUI Settings Panel**
**Decision**: Edit config.json directly or via setup wizard, no runtime prefs UI
**Rationale**:
- Simplifies UI: fewer settings panels
- Config is rarely changed after setup
- Versioning: config.json can be git-tracked for team consistency
- Setup wizard handles 95% of first-run needs

---

## Component Interface Contracts

### FastAPI Request/Response Format

**File Read**:
```typescript
// Request
GET /api/files/read?path=/mnt/nobility-vault/projects/myproj/src/main.rs

// Response (200 OK)
{
  "path": "/mnt/nobility-vault/projects/myproj/src/main.rs",
  "content": "fn main() { ... }",
  "encoding": "utf-8",
  "size_bytes": 1024,
  "last_modified": 1704067200
}

// Error (400 Bad Request - path outside whitelist)
{
  "error": "Path not in allowed roots",
  "requested": "/etc/passwd"
}
```

**Model Inference (streaming SSE)**:
```typescript
// Request
POST /api/qwen/14b
{
  "messages": [
    { "role": "user", "content": "Hello" }
  ],
  "context_files": [
    { "path": "/mnt/nobility-vault/projects/myproj/README.md", "inline": true }
  ]
}

// Response (text/event-stream)
data: {"choices":[{"delta":{"content":"Hello"}}]}
data: {"choices":[{"delta":{"content":" there"}}]}
data: {"choices":[{"delta":{"content":"!"}}]}
data: [DONE]
```

**Git Commit**:
```typescript
// Request
POST /api/git/commit
{
  "project": "myproject",
  "message": "edit: Update main.rs"
}

// Response (200 OK)
{
  "hash": "a1b2c3d4",
  "message": "edit: Update main.rs",
  "timestamp": 1704067200,
  "author": "hunt <hunt@local>"
}
```

---

## Error Handling Strategy

**Tier 1: FastAPI Errors**
```python
@app.exception_handler(HTTPException)
async def http_exception_handler(request, exc):
    return JSONResponse(
        status_code=exc.status_code,
        content={
            "error": exc.detail,
            "timestamp": time.time(),
            "request_id": request.headers.get("X-Request-ID")
        }
    )

# Path validation error
raise HTTPException(
    status_code=400,
    detail="Path not in allowed roots"
)
```

**Tier 2: React Error Boundaries**
```typescript
// App.tsx wraps in error boundary
<ErrorBoundary fallback={<ErrorPage />}>
  <ChatPanel />
</ErrorBoundary>

// On error: display toast with retry button
toast.error("Failed to load file", {
  action: <button onClick={retryLoadFile}>Retry</button>
})
```

**Tier 3: User Feedback**
- Network error → "Backend unavailable. Retrying..." (auto-retry every 3s)
- Model endpoint down → "Qwen 14B not responding. Start model service?"
- File not found → inline warning badge on file reference
- Git push failed → "Sync pending (network offline). Will retry automatically."

---

## Correctness Properties

*A property is a universal characteristic that should hold true across all executions of the system. Properties bridge human specifications and machine-verifiable correctness.*

### Property 1: File Path Validation

**For all** filesystem operations (read, write, delete), if the requested path is outside `/mnt/nobility-vault/` or `/home/hunt/`, **then** the operation SHALL be rejected with a 400 error before any filesystem access occurs.

**Validates: Requirements 13.5, 9.1 (security: path traversal prevention)**

### Property 2: Message History Round-Trip

**For any** message sent by the user in PLAN IT mode, when it is written to `.history/messages.jsonl` and subsequently loaded on project reopening, the message content, sender role, and timestamp SHALL be identical to the original.

**Validates: Requirements 5.2, 5.1 (persistence: no data loss)**

### Property 3: File Tree Consistency

**For any** file created in the project directory (via UI, command, or external process), the FileTree component SHALL reflect the new file within 100ms of the creation event, without requiring user interaction.

**Validates: Requirements 7.4, 14.2, 14.3 (real-time sync)**

### Property 4: Git Commit Atomicity

**For any** user action that triggers auto-commit (save file, create spec, generate tasks), if the git commit succeeds, then the local git repository SHALL contain a commit with the expected message, and the file(s) SHALL be staged and committed atomically in a single transaction.

**Validates: Requirements 10.1-10.4 (no partial commits)**

### Property 5: Model Endpoint Failover

**For any** request to a model endpoint that is unavailable (e.g., localhost:8000 down), the application SHALL detect the failure within 5 seconds (via health check timeout), disable the model selector for that model, display an error banner, and re-enable the selector when the endpoint becomes available again.

**Validates: Requirements 2.3, 2.4, 2.5 (graceful degradation)**

### Property 6: File Reference Resolution

**For any** file reference syntax in a chat message (e.g., `@/path/to/file.txt` or `@relative/path`), if the file exists at the resolved path, then the file content SHALL be read and included in the model request context; if the file does not exist, then an inline warning badge SHALL appear on the reference and the message SHALL be sent without the file content.

**Validates: Requirements 3.2-3.4 (file context accuracy)**

### Property 7: Configuration Persistence

**For any** setting saved during setup wizard (project vault path, model endpoints, theme), the application SHALL write the setting to `/mnt/nobility-vault/.config/qwen-kiro/config.json`, and on subsequent application launches, the setting SHALL be loaded and applied without user interaction.

**Validates: Requirements 18.2, 20.2 (no configuration loss)**

### Property 8: WebSocket Reconnection

**For any** WebSocket connection to the FastAPI backend that is interrupted, the application SHALL automatically attempt to reconnect every 2 seconds until successful; during disconnection, the File Tree SHALL fall back to HTTP polling every 5 seconds to detect changes.

**Validates: Requirements 14.4, 14.5 (resilience: always-on updates)**

### Property 9: Command Execution Safety

**For any** shell command executed via the Terminal Pane, the application SHALL:
1. Display the command in a confirmation dialog before execution
2. Execute the command in the current project working directory (not system-wide paths)
3. Capture and display stdout and stderr in real-time
4. Display the exit code if non-zero

**Validates: Requirements 4.2-4.5 (safe command execution)**

### Property 10: Spec Workflow Idempotency

**For any** spec feature name, calling "Generate Tasks" twice on the same spec content SHALL produce identical tasks.md output (same task IDs, descriptions, and ordering), ensuring deterministic workflow.

**Validates: Requirements 8.3, 8.4 (reproducibility)**

### Property 11: Git Commit Message Consistency

**For any** file operation (save, spec create, code generate), the git commit message SHALL follow the convention: `{action}: {resource_name}` (e.g., `edit: main.rs`, `spec: user-auth`, `gen: Implement task 1`), making commit history human-readable and parseable.

**Validates: Requirements 10.1-10.4 (audit trail clarity)**

### Property 12: Mode State Preservation

**For any** switch from PLAN IT mode to ENGINEER IT mode and back, the application SHALL preserve:
- Current scroll position in chat/file editor
- Message history (no loss of prior messages)
- File tree expansion state (which folders are open/closed)
- Current file selection

**Validates: Requirements 6.4, 5.3 (UX: no context loss on mode toggle)**

---

## Integration Points and Dependencies

### External Service Dependencies
1. **Qwen 14B Model** (localhost:8000/v1) — OpenAI-compatible API
2. **Qwen Coder Model** (localhost:8001/v1) — OpenAI-compatible API
3. **GitHub/Hugging Face** (remote git push) — optional, offline operation still works
4. **Git binary** — must be installed on system PATH
5. **Python 3.11+** — FastAPI backend runtime

### Component Dependencies
- React 18, TypeScript, Tauri 1.6+, Monaco Editor, Prism.js, python-watchdog, FastAPI, httpx

---

## Testing Strategy

**Unit Tests** (per component):
- FileReferenceParser: Does @-syntax correctly resolve paths?
- PathValidator: Do non-whitelisted paths get rejected?
- MessageHistoryLoader: Does JSONL parsing handle corrupted lines?
- GitCommitFormatter: Is commit message format consistent?

**Property-Based Tests** (universal properties):
- File tree consistency: random file creates/deletes, verify tree reflects state within 100ms
- Message round-trip: serialize/deserialize random messages, verify content is identical
- Git atomicity: concurrent file saves, verify each creates exactly one commit
- Model failover: simulate endpoint failures, verify health check detects within 5s

**Integration Tests**:
- End-to-end spec workflow: create spec → generate tasks → generate code → verify files written
- Chat + command execution: send message → run command → verify output appears in terminal
- File sync: external process writes file → verify FileTree updates within 100ms

**Manual Smoke Tests**:
- First-run setup wizard completes without errors
- PLAN IT mode: chat with both models, switch between them
- ENGINEER IT mode: create file, delete file, see updates in FileTree
- Mode toggle: switch PLAN IT ↔ ENGINEER IT, verify context preserved
- Git: verify commits created with correct messages, inspect .git/logs
- Offline operation: disconnect network, verify local operations continue

---

## Deployment and Packaging

**Desktop Installation**:
- Tauri builder generates AppImage (Ubuntu) or .deb package
- Installer script creates `/mnt/nobility-vault/qwen-kiro-ide/` directory
- Desktop file (.qwen association) installed to `~/.local/share/applications/`
- First-run setup wizard initializes config

**Production Build**:
```bash
# Build React frontend (optimized)
npm run build

# Build Tauri app (releases AppImage + .deb)
cargo tauri build

# Output: src-tauri/target/release/bundle/
```

**Runtime Dependencies**:
- Python 3.11+ (FastAPI + watchdog)
- Node.js 18+ (React build, not runtime)
- Git (system command)
- WebView2/GTK (provided by Tauri)

---

## Future Enhancements (Out of Scope)

1. **Remote FastAPI**: Move backend to cloud for team collaboration
2. **Advanced Git UI**: Merge conflict resolution, branch switching visual UI
3. **Code Diffing**: Visual diff viewer for file changes
4. **LLM Fine-Tuning**: Local model training on project code
5. **GPU Acceleration**: CUDA integration for model inference speedup
6. **IDE Extensions**: Plugin system for custom components
7. **Real-time Collaboration**: Multi-user editing (similar to Figma)

