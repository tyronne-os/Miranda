# Requirements Document: Qwen Kiro Desktop IDE

## Introduction

Qwen Kiro Desktop IDE is a dual-mode desktop application that combines conversational AI planning (PLAN IT) with integrated development engineering (ENGINEER IT). It provides developers with a cohesive offline environment for spec generation, code ideation, and project management, powered by locally-running Qwen 14B and Qwen Coder models.

## Glossary

- **PLAN IT Mode**: Conversational interface for AI-assisted reasoning, spec writing, and command execution
- **ENGINEER IT Mode**: Project-focused interface with file explorer, spec workflow, and code generation
- **Qwen 14B**: Locally-running language model for reasoning and spec breakdown (inference endpoint: localhost:8000/v1)
- **Qwen Coder**: Locally-running code generation model (inference endpoint: localhost:8001/v1)
- **Project Vault**: Primary directory for user projects at /mnt/nobility-vault/projects/
- **Spec Workflow**: Three-stage process: spec creation → task generation → code generation
- **Message History**: Persistent record of conversation turns within a project context
- **Model Endpoint**: Local HTTP(S) inference server providing OpenAI-compatible API
- **File Reference**: User notation (@/path/to/file or @project_name) to include files in AI context
- **Git Commit**: Automatic atomic commit to local git repository on spec/code events
- **Desktop Integration**: Operating system menu and desktop file entries for application launch

## Requirements

### Requirement 1: PLAN IT Mode Chat Interface

**User Story:** As a developer, I want a conversational chat interface identical to Claude Desktop so that I can reason through problems interactively without context switching.

#### Acceptance Criteria

1. THE PLAN IT Mode SHALL display a message history panel on the right side with all user and assistant messages in chronological order
2. THE PLAN IT Mode SHALL provide a text input field at the bottom of the chat panel that accepts multi-line message composition
3. WHEN a user presses Enter or clicks Send, THE Chat Interface SHALL transmit the message to the active Qwen model via the Model Endpoint
4. THE Chat Interface SHALL display assistant responses in real-time as they arrive from the Model Endpoint
5. WHERE syntax highlighting is applicable, THE Chat Interface SHALL apply markdown rendering and code block syntax highlighting to assistant responses
6. THE Chat Interface SHALL maintain scroll position at the latest message and auto-scroll when new messages arrive
7. WHEN a user clicks a file reference button or code block copy icon, THE Chat Interface SHALL execute the requested action (copy to clipboard, open file) without page navigation

---

### Requirement 2: Qwen Model Selection and Switching

**User Story:** As a developer, I want to switch between Qwen 14B and Qwen Coder mid-conversation so that I can choose the right model for each reasoning step.

#### Acceptance Criteria

1. THE Application SHALL display a model selector dropdown in the PLAN IT Mode header
2. WHEN the user selects a model from the dropdown, THE Application SHALL route subsequent messages to the selected Model Endpoint (localhost:8000/v1 for Qwen 14B, localhost:8001/v1 for Qwen Coder)
3. WHEN a model endpoint is unavailable at startup, THE Application SHALL display a warning badge on that model's selector option and disable selection until the endpoint becomes available
4. THE Application SHALL check model endpoint availability on startup and every 5 minutes thereafter
5. IF both model endpoints are unavailable, THE Application SHALL display an error banner in the UI and disable the Chat Interface until at least one endpoint is reachable

---

### Requirement 3: File Reference and Context Awareness

**User Story:** As a developer, I want to reference project files and folders in chat messages so that the AI can see the code I'm discussing.

#### Acceptance Criteria

1. THE Chat Interface SHALL recognize file reference syntax: @/absolute/path/to/file or @project_name
2. WHEN a user types @, THE Chat Interface SHALL display an autocomplete dropdown populated with recent files from the current project, recent project names, and filesystem paths matching the typed prefix
3. WHEN a user selects or completes a file reference, THE Chat Interface SHALL retrieve the file content from the filesystem and include it in the model request context
4. IF a referenced file does not exist, THE Chat Interface SHALL display an inline warning badge on the reference and continue sending the message without the file content
5. THE Application SHALL track recently accessed files and projects to populate the autocomplete suggestions

---

### Requirement 4: Code Execution in Terminal Pane

**User Story:** As a developer, I want to run Python and shell commands directly from PLAN IT Mode so that I can test ideas without switching applications.

#### Acceptance Criteria

1. THE PLAN IT Mode SHALL provide a Terminal Pane below the chat history panel that displays command output
2. WHEN an assistant message contains a shell or Python code block marked with ```bash, ```sh, or ```python, THE Chat Interface SHALL render an adjacent "Run" button
3. WHEN a user clicks the "Run" button, THE Application SHALL display a confirmation dialog stating the command to be executed
4. WHEN a user confirms, THE Terminal Pane SHALL execute the command in the current working directory and display stdout/stderr output in real-time
5. IF command execution fails or returns a non-zero exit code, THE Terminal Pane SHALL display the exit code and stderr in red text
6. THE Terminal Pane SHALL maintain command history accessible via up/down arrow keys for manual command entry

---

### Requirement 5: Persistent Message History per Project

**User Story:** As a developer, I want my chat conversations to persist across sessions so that I can resume work without losing context.

#### Acceptance Criteria

1. WHEN a user sends a message in PLAN IT Mode, THE Application SHALL write the message and assistant response to a project-local message history file at /mnt/nobility-vault/projects/{project_name}/.history/messages.jsonl
2. WHEN a user opens PLAN IT Mode for an existing project, THE Application SHALL load all previous messages from the history file and display them in chronological order
3. WHEN the user scrolls up in the message history, THE Application SHALL load and display earlier messages without network latency
4. WHEN a user switches to ENGINEER IT Mode and back to PLAN IT Mode, THE Chat Interface SHALL restore the scroll position to where the user was before switching
5. IF the history file is corrupted or unreadable, THE Application SHALL display a warning and start a fresh conversation without loading prior messages

---

### Requirement 6: ENGINEER IT Mode UI and Layout

**User Story:** As a developer, I want a dedicated ENGINEER IT Mode with a project explorer so that I can manage code and specs without chat distraction.

#### Acceptance Criteria

1. THE Application header SHALL display a toggle button labeled "PLAN IT / ENGINEER IT"
2. WHEN the user clicks the toggle, THE Application SHALL switch to ENGINEER IT Mode, displaying a left-side project explorer panel and right-side spec/chat panel
3. THE Layout in ENGINEER IT Mode SHALL be identical in structure to the web version: collapsible project explorer on the left, full-height spec panel on the right
4. WHEN a user toggles between modes, THE Application SHALL preserve the current scroll position, message history, and file selection state
5. THE Mode toggle SHALL display the current active mode with visual emphasis (bold text or icon highlight)

---

### Requirement 7: Real-Time File Tree and Project Explorer

**User Story:** As a developer, I want a live file tree that updates when files change so that I always see the current project structure.

#### Acceptance Criteria

1. THE Project Explorer SHALL display a hierarchical file tree rooted at /mnt/nobility-vault/projects/{project_name}/
2. THE File Tree SHALL show directories and files with appropriate icons (folder, file, code file, etc.)
3. THE File Tree nodes SHALL be collapsible/expandable by clicking a chevron icon
4. WHEN a file is created, modified, or deleted in the project directory, THE File Tree SHALL update within 100ms without requiring user action or page refresh
5. WHEN a user right-clicks on a file or directory in the File Tree, THE Application SHALL display a context menu with options: Create File, Create Folder, Rename, Delete, Open in Terminal
6. WHEN a user clicks on a file in the File Tree, THE Application SHALL display the file content in a read-only preview pane or text editor in the right panel

---

### Requirement 8: Spec Workflow Integration

**User Story:** As a developer, I want a three-stage spec workflow so that I can generate tasks and code from specs systematically.

#### Acceptance Criteria

1. WHEN a user opens ENGINEER IT Mode for a project, THE Application SHALL display a "New Spec" button in the header
2. WHEN a user clicks "New Spec", THE Application SHALL prompt for a feature name and create a new spec directory at /mnt/nobility-vault/projects/{project_name}/.kiro/specs/{feature_name}/
3. WHEN a user writes a spec description and clicks "Generate Tasks", THE Application SHALL send the spec to Qwen 14B and parse the response to generate a tasks.md file
4. WHEN a user reviews tasks and clicks "Generate Code", THE Application SHALL send each task to Qwen Coder and generate implementation files in the project directory
5. THE Spec Workflow shall maintain an audit trail in the project history so the user can review what was generated and when

---

### Requirement 9: File Operations (Create, Read, Update, Delete)

**User Story:** As a developer, I want full CRUD file operations in the IDE so that I can manage project files without external tools.

#### Acceptance Criteria

1. THE Application SHALL provide keyboard shortcuts: Ctrl+N (New File), Ctrl+Shift+N (New Folder), Ctrl+S (Save), Ctrl+Shift+S (Save As)
2. WHEN a user creates a new file or folder, THE Application SHALL accept a name input and create the filesystem entry at /mnt/nobility-vault/projects/{project_name}/{user_provided_path}
3. WHEN a user opens a file in the editor, THE Application SHALL read the file content and display it in a text editor with syntax highlighting matching the file extension
4. WHEN a user edits a file and presses Ctrl+S, THE Application SHALL write the changes to the filesystem and display a confirmation message "Saved"
5. WHEN a user right-clicks a file and selects Delete, THE Application SHALL prompt for confirmation, then delete the file and update the File Tree
6. IF a file is modified externally (by another process), THE Application SHALL detect the change and prompt the user to reload or merge the external changes

---

### Requirement 10: Git Integration and Auto-Commit

**User Story:** As a developer, I want automatic git commits on key events so that my work is always backed up without manual steps.

#### Acceptance Criteria

1. WHEN a user creates a new spec in ENGINEER IT Mode, THE Application SHALL create a git commit with message "spec: Create {feature_name} spec"
2. WHEN a user saves a file in the text editor, THE Application SHALL create a git commit with message "edit: Update {file_name}"
3. WHEN the Spec Workflow generates a tasks.md file, THE Application SHALL create a git commit with message "gen: Generate tasks for {feature_name}"
4. WHEN the Spec Workflow generates code, THE Application SHALL create a git commit with message "gen: Implement {task_name}"
5. IF git is not initialized in the project directory, THE Application SHALL run `git init` and create an initial commit before the first auto-commit
6. THE Application SHALL display the git commit hash and timestamp in the status bar after each auto-commit

---

### Requirement 11: GitHub and Hugging Face Synchronization

**User Story:** As a developer, I want my projects auto-synced to GitHub or Hugging Face so that my work is backed up to external storage.

#### Acceptance Criteria

1. WHEN a user opens a project for the first time, THE Application SHALL prompt for a remote repository URL (GitHub HTTPS or Hugging Face git URL)
2. WHEN a user provides a remote URL, THE Application SHALL add it as a git remote named "origin" via `git remote add origin {url}`
3. WHEN a git commit is created locally, THE Application SHALL push it to the remote origin via `git push origin main` within 5 seconds of the commit
4. IF the push fails due to network unavailability, THE Application SHALL queue the push and retry every 30 seconds until successful
5. IF the push fails due to authentication error, THE Application SHALL display an error banner with a link to configure credentials and disable auto-push until credentials are provided
6. THE Application SHALL display the remote repository URL in the status bar so the user can click it to open the repo in a browser

---

### Requirement 12: Application Installation and Desktop Integration

**User Story:** As a developer, I want to install Qwen Kiro Desktop IDE with a single installer so that it appears in my applications menu and can open .qwen project files.

#### Acceptance Criteria

1. THE Installer SHALL create the application directory at /mnt/nobility-vault/qwen-kiro-ide/
2. THE Installer SHALL create a desktop shortcut file at ~/.local/share/applications/qwen-kiro.desktop with name "Qwen Kiro" and category "Development"
3. THE Installer SHALL register .qwen as a project file extension, so double-clicking a .qwen file opens it in Qwen Kiro
4. WHEN Qwen Kiro is first launched, THE Application SHALL display a setup wizard that prompts the user to select a default project vault (default: /mnt/nobility-vault/projects/)
5. WHEN Qwen Kiro is first launched, THE Application SHALL display a setup wizard that prompts the user for model paths or endpoints (default: localhost:8000/v1 and localhost:8001/v1)
6. THE Setup Wizard SHALL provide a "Test Connection" button that verifies each model endpoint is reachable before allowing the user to proceed

---

### Requirement 13: FastAPI Backend Server

**User Story:** As a developer, I want the desktop app to communicate with a FastAPI backend so that it can handle file I/O, git operations, and model inference without blocking the UI.

#### Acceptance Criteria

1. THE Application SHALL start a FastAPI server on localhost:8002 automatically on application startup
2. THE FastAPI server SHALL provide REST endpoints for: list projects, create project, read file, write file, delete file, list files in directory
3. THE FastAPI server SHALL execute git commands via subprocess and return the output to the client
4. WHEN the desktop app needs to read or write a file, THE Application SHALL make an HTTP request to the FastAPI backend instead of direct filesystem access
5. WHEN the FastAPI server receives a file operation request, THE Server SHALL verify the requested path is within /mnt/nobility-vault/ or /home/hunt/ before executing the operation
6. IF the FastAPI server crashes, THE Application SHALL display an error and automatically restart it within 5 seconds

---

### Requirement 14: WebSocket Real-Time File Updates

**User Story:** As a developer, I want real-time file tree updates via WebSocket so that changes from external processes appear instantly.

#### Acceptance Criteria

1. THE Application SHALL establish a WebSocket connection to the FastAPI backend on startup
2. WHEN a file is created, modified, or deleted in the project directory, THE FastAPI backend SHALL detect the change via file system watcher and emit a WebSocket event
3. WHEN the desktop app receives a file update event, THE File Tree SHALL refresh the affected branch without reloading the entire tree
4. WHEN the WebSocket connection is interrupted, THE Application SHALL attempt to reconnect every 2 seconds until successful
5. WHILE the WebSocket connection is closed, THE File Tree SHALL fall back to polling the filesystem every 5 seconds to detect changes

---

### Requirement 15: Offline Operation and Graceful Degradation

**User Story:** As a developer, I want the application to work fully offline so that I can develop without an internet connection.

#### Acceptance Criteria

1. THE Application SHALL not require internet access for core functionality (PLAN IT Mode chat, ENGINEER IT Mode file editing, code execution)
2. WHEN a model endpoint is unavailable, THE Application SHALL disable the chat interface and display an error message with instructions to start the model service
3. WHEN GitHub/Hugging Face sync fails due to network unavailability, THE Application SHALL queue the sync and retry on network reconnection
4. WHEN a user attempts to push to a remote repository while offline, THE Application SHALL display a warning and continue working locally, retrying the push later
5. IF a user disables remote sync in preferences, THE Application SHALL continue all other functionality (file editing, code execution, local git commits) without change

---

### Requirement 16: Dark Theme and Visual Design

**User Story:** As a developer, I want a VS Code-style dark theme so that the application is easy on the eyes during extended use.

#### Acceptance Criteria

1. THE Application SHALL use a dark color scheme (background dark gray #1e1e1e, text white #e0e0e0, accent blue #007acc) matching VS Code's built-in theme
2. WHEN the user toggles between PLAN IT and ENGINEER IT modes, THE Transition SHALL be smooth (200ms fade or slide animation)
3. THE File Tree nodes SHALL highlight on hover and show the current selection with a subtle background color
4. CODE BLOCKS in chat messages SHALL use syntax highlighting with colors matching the selected theme
5. THE Application SHALL provide a preferences panel where the user can select between light and dark themes (dark theme is default)

---

### Requirement 17: Project Switching and Recent Projects

**User Story:** As a developer, I want quick access to recent projects so that I can switch between them without navigation.

#### Acceptance Criteria

1. THE Application header SHALL display a project selector dropdown showing the currently open project
2. WHEN the user clicks the project selector, THE Dropdown SHALL display a list of recent projects (last 10 accessed) and a "Browse All Projects" option
3. WHEN a user selects a project from the dropdown, THE Application SHALL close the current project and open the selected project, loading its file tree and message history
4. WHEN the Application starts, IF no project is specified on the command line, THE Application SHALL display the recent projects list and prompt the user to select one
5. THE Application SHALL display the full path of the current project in the window title bar

---

### Requirement 18: First-Run Setup and Configuration

**User Story:** As a developer, I want a guided first-run setup so that the application is ready to use without manual configuration.

#### Acceptance Criteria

1. WHEN Qwen Kiro launches for the first time, THE Application SHALL display a setup wizard with the following steps in order:
   - Select default project vault directory (default: /mnt/nobility-vault/projects/)
   - Configure Qwen 14B model endpoint (default: localhost:8000/v1)
   - Configure Qwen Coder model endpoint (default: localhost:8001/v1)
   - Test model endpoint connectivity
   - Choose default theme (light or dark)
2. WHEN the user completes the setup wizard, THE Application SHALL save settings to /mnt/nobility-vault/.config/qwen-kiro/config.json
3. WHEN the user clicks "Test Connection" for a model endpoint, THE Application SHALL make an HTTP request to the endpoint and display "Connected" or "Failed to connect"
4. IF the setup wizard is closed without completion, THE Application SHALL display it again on the next launch

---

### Requirement 19: Keystroke Shortcuts and Navigation

**User Story:** As a developer, I want keyboard shortcuts for common actions so that I can work without touching the mouse.

#### Acceptance Criteria

1. THE Application SHALL support the following keyboard shortcuts:
   - Ctrl+N: New File
   - Ctrl+Shift+N: New Folder
   - Ctrl+S: Save active file
   - Ctrl+Shift+S: Save As (rename)
   - Ctrl+K Ctrl+O: Open project folder
   - Ctrl+B: Toggle File Tree visibility
   - Ctrl+`: Toggle Terminal Pane visibility
   - Ctrl+Tab: Switch to next open file tab
   - Ctrl+Shift+Tab: Switch to previous open file tab
   - Ctrl+P: Quick file opener (fuzzy search file tree)
   - Alt+1: Switch to PLAN IT Mode
   - Alt+2: Switch to ENGINEER IT Mode
2. WHEN a user presses Ctrl+P, THE Application SHALL display a quick search overlay that filters files in the current project by substring matching
3. THE Shortcut keys SHALL be displayed in menus and tooltips so the user can discover them

---

### Requirement 20: Data Storage and Configuration

**User Story:** As a developer, I want all my project data and settings to be stored locally so that I maintain full control and can version control everything.

#### Acceptance Criteria

1. THE Application SHALL store all projects in /mnt/nobility-vault/projects/ by default (user-configurable)
2. THE Application SHALL store application configuration in /mnt/nobility-vault/.config/qwen-kiro/config.json
3. THE Application SHALL store message history in {project_dir}/.history/messages.jsonl
4. THE Application SHALL store spec artifacts in {project_dir}/.kiro/specs/{feature_name}/ (requirements.md, design.md, tasks.md)
5. WHEN a user requests a backup, THE Application SHALL run `git push origin main` to sync all data to the configured remote repository

