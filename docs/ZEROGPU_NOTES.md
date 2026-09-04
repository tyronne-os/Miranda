# ZeroGPU (HF Pro) — usage decisions for Model Forge

## What ZeroGPU actually is (verified against HF docs)
- Dynamically attaches an NVIDIA Blackwell/H200 slice to a **Gradio** Space
  only around a `@spaces.GPU`-decorated function call, then releases it.
- Pro tier: ~40 min/day of GPU time.
- **Cannot** run a persistent GPU server process (no ComfyUI-style
  long-running server, no vLLM daemon), and **cannot** be used to offload
  local compilation (e.g. `cargo build`) — it is function-scoped and
  Gradio-bound.

## Where we use it
Model Forge Tasks 6 and 7 only:
- Task 6: LoRA fine-tune runs (`peft`)
- Task 7: model merges (`mergekit`)

These were the two tasks flagged as unverifiable locally (no GPU). Everything
else in Phase One is CPU-only and stays local.

## Execution mode: RAW / EAGER ONLY
Per explicit project decision:
- **No** `torch.compile`
- **No** ahead-of-time compilation (AOTI / `spaces.aoti_*` helpers)
- Plain eager PyTorch inside the `@spaces.GPU` function.

Rationale: AOT compilation is the main source of opaque cold-start compile
failures on ZeroGPU, and for LoRA training / weight merging the throughput
gain does not justify the fragility. Fine-tuning and merging are dominated by
I/O and optimizer steps, not by kernel launch overhead.

## Credentials
HF token is already vaulted (provider `huggingface`, status ok) and retrieved
via the Access vault MCP server — never pasted or committed.
