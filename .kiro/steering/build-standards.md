# Miranda-Engine build standards

These rules apply to every Work Order in `.kiro/specs/` and any other work in this repo.

## No claimed progress without evidence

A Work Order is not "done" because the code was written — it's done when there's real command output proving it: `cargo build`/`cargo test` passing, a real measured latency number, a real screen-capture/frame-diff showing the No Loop Video Protocol is satisfied. Never report a task complete on code-review confidence alone.

## No simulated inference, ever

If a component doesn't have a real backend wired up yet (an unimplemented Node Warden, an unrigged Gaussian-splat asset), say so plainly rather than faking output. This project's own prior work (`eve-ecc-docs/ORCHESTRATION-PIVOT.md`) documents exactly this failure once already — a placeholder scored 96/100 because the measurement read code intent, not painted pixels. Don't repeat it.

## Reuse before rebuild

Before writing new code, check `client-apps/web`, `client-services/ace-controller`, and the existing Rust crate stubs for something already close to what's needed. The first attempt (eve-ecc) has real, working pieces — extend them.

## GPU cost discipline

Per the `aws-pipeline-architect` skill: never leave a GPU instance running idle. WO-1 through WO-4 need zero GPU. Only WO-5's actual rendering/rigging work touches a GPU instance, and only for the duration of the specific test.

## Cross-reference the Kiro skills, don't re-derive their content

`nobility-posh-framework`, `live-avatar-expert`, `aws-pipeline-architect`, and `llamacpp-huggingface-expert` are all active global skills with the science, architecture rationale, and deployment rules already written out. Reference them; don't duplicate or re-research what they already contain.

## Podman: hybrid placement, not blanket containerization

Standardize on rootless Podman for WO-2 (Nemotron routing), WO-4 (WebRTC transport), and WO-5 (React Flow UI) — real portability (identical container runs on this machine and AWS ECR/Fargate), real blast-radius containment (a broken `pip install` inside an agent-driven build destroys a container, not the host), real security benefit (rootless by default).

**WO-1's `miranda-ipc` crate and any direct GPU rendering path stay bare-metal on the EC2 instance**, not containerized — this is the one deliberate exception, made for the sub-150ms latency target. Note on *why*, precisely: a bind-mounted `tmpfs` file (`/dev/shm/miranda_bus`) shares the same physical pages across a container boundary, so `mmap` on it is still genuinely zero-copy — the mount itself is not inherently a latency tax. The real, more modest overhead is container-runtime syscall interception (seccomp filtering, cgroup accounting) at the mmap/futex call sites. That overhead is real but not dramatic — treat "keep WO-1 bare-metal" as the conservative default worth empirically re-measuring once the ring buffer is built and benchmarked, not as settled physics that forecloses ever revisiting it.

## 4. Ephemeral Session Isolation (Podman)

The `miranda-supervisor` crate must manage workflow test execution exclusively via ephemeral, rootless Podman containers:

- Spawn a fresh Podman container per unique testing session — pristine, zero-state, no lingering cache or port conflicts from a prior run.
- Volume-mount `/dev/shm/miranda_bus` into the session container (`-v /dev/shm/miranda_bus:/dev/shm/miranda_bus:rw`) so the containerized session can still reach the bare-metal IPC bus with zero-copy semantics (see the note above on why this mount doesn't itself cost latency).
- Enforce strict container lifecycle management: graceful teardown on session end, forced `SIGKILL` + `podman rm -f` on timeout — no zombie processes, no resource starvation across parallel sessions.
- This is what makes Miranda a true multi-tenant testing lab, not just a renderer: many workflow tests can run in parallel on one EC2 instance, each in its own disposable container, all sharing the one bare-metal IPC core.

## 5. The "Power of Pivoting" (Anti-Loop Protocol)

You are strictly forbidden from engaging in repetitive debugging loops. If you encounter the same compilation error, logic failure, or test failure on a specific block of code for the third consecutive attempt, you must immediately cease local troubleshooting. You are fully empowered to pivot.

When the 3-Strike limit is reached, execute the following Pivot Protocol:

**Step 1: Halt and Search.** Abandon the current approach. Utilize your web search and repository scanning capabilities to query Hugging Face, GitHub, or arXiv for alternative solutions.

**Step 2: Source High-Tier Reference Code.** Filter your research for working, reproducible code from top-rated projects, heavily starred repositories, or recent peer-reviewed papers (2025-2026). Do not pull from obscure or unverified sources.

**Step 3: Rip and Replace.** Do not attempt to forcefully merge the new solution with the broken logic. Completely replace the failing implementation with the verified architectural pattern you discovered.

**Step 4: Document the Pivot.** When you successfully implement the new solution, explicitly state in the chat: "PIVOT EXECUTED." You must provide the URL of the GitHub repository or Hugging Face paper you pulled the working architecture from, along with a one-sentence rationale of why it bypassed the error.
