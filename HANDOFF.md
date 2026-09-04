# New Toy — Miranda Engine Handoff Guide

**Project:** NVIDIA Tokkio + AWS Bedrock/Polly/Transcribe + GCP Qwen 14B real-time digital human pipeline  
**Status:** Alpha (Pipeline 1 — AWS-native services + GCP LLM backend)  
**Last Updated:** Aug 27, 2026  
**Deployed Stack:** GCP berylize-node (g2-standard-4, NVIDIA L4, $0.40/hr)

---

## Quick Start

### Prerequisites

1. **Node.js 18+** — for the web client and AWS SDK calls
2. **Rust 1.70+** — for the IPC harness (optional for testing on CPU)
3. **Docker** — for containerized Tokkio runtime (optional)
4. **AWS Account** with credentials for:
   - Amazon Transcribe Streaming (ASR)
   - Amazon Bedrock (LLM — Claude or Nova Pro)
   - Amazon Polly Neural TTS
5. **GCP Account** with access to berylize-node instance (34.26.220.164)
6. **Hugging Face token** (for model hub access)
7. **NVIDIA NGC API key** (for Tokkio container pull)

### Environment Setup

Clone the repo and set up your `.env` file:

```bash
git clone https://github.com/tyronne-os/new-toy.git
cd new-toy
cp .env.example .env
```

Fill in `.env` with your credentials:

```env
# AWS
AWS_ACCESS_KEY_ID=<your_key>
AWS_SECRET_ACCESS_KEY=<your_secret>
AWS_REGION=us-east-1

# GCP (for LLM backend — Qwen 14B on berylize-node)
GCP_INSTANCE_IP=34.26.220.164
GCP_QWEN_PORT=8000
GCP_VOICE_AGENT_PORT=8081

# NVIDIA
NGC_API_KEY=<your_ngc_key>
NVIDIA_NIM_API_KEY=<your_nim_key>

# Hugging Face
HUGGINGFACE_TOKEN=<your_hf_token>

# GitHub (for versioning)
GITHUB_TOKEN=<your_github_token>
```

### Fire Up the Pipeline

#### 1. Start the Web Client (React + Vite)

```bash
cd client-apps/web
npm install
npm run dev
```

The UI will be live at **http://localhost:5173**

#### 2. Open the Settings Panel

- Click the **⚙️ gear icon** in the top-right corner
- Enter your **API keys** in the Key Vault form
- Verify connectivity with the green/red indicators

#### 3. Start the AWS Transcribe Listener (ASR ingress)

In a new terminal:

```bash
cd client-services
node transcribe-stream-client.mjs
```

This opens a WebSocket to Amazon Transcribe Streaming. Your browser microphone will stream audio → real-time transcript.

#### 4. Start the GCP Qwen LLM Backend (if not already running)

The berylize-node instance (34.26.220.164) should already have llama-server running on port 8000. Verify:

```bash
curl http://34.26.220.164:8000/v1/models
```

You should see the Qwen model listed.

#### 5. Wire Polly TTS + Viseme Mapping

The pipeline automatically:
- Takes the transcript from step 3
- Sends it to GCP Qwen on berylize-node for a response
- Pipes the response to **Amazon Polly Neural TTS** with `SpeechMarkTypes=['viseme']`
- Maps Polly visemes to ARKit blend shape weights
- Pushes the frame stream to the browser canvas

#### 6. Render with Amazon Sumerian Hosts (or Tokkio)

The right-side canvas in THE VANITY UI renders the animated avatar in real-time:
- **Sumerian Hosts** (Three.js) for instant preview — works on CPU
- **Tokkio Docker** (optional) — for full-fidelity NVIDIA rendering on GPU

To use Tokkio Docker (requires NGC API key):

```bash
docker pull nvcr.io/nvidia/tokkio/tokkio:latest
docker run --gpus all \
  -e NGC_API_KEY=$NGC_API_KEY \
  -p 8080:8080 \
  nvcr.io/nvidia/tokkio/tokkio:latest
```

Then point the canvas renderer to `http://localhost:8080/stream` in the UI settings.

---

## Architecture Overview

```
[Browser Microphone]
         ↓
[Amazon Transcribe Streaming] ← Real-time ASR (WebSocket)
         ↓ (WebSocket → audio stream)
[Transcript JSON] → Miranda IPC bus
         ↓
[GCP berylize-node:8000 — Qwen 14B LLM]
         ↓ (LLM response text)
[Amazon Polly Neural TTS + Speech Marks] ← Viseme events (JSON)
         ↓ (Polly viseme → ARKit blend shape weights)
[BlendshapeFrame] → Miranda IPC bus
         ↓
[Amazon Sumerian Hosts / Tokkio Docker] ← Animated avatar renderer
         ↓
[THE VANITY UI Canvas] ← Real-time live video stream
```

### Key Services

| Service | Role | Provider | Cost/Status |
|---------|------|----------|------------|
| **Transcribe Streaming** | ASR (speech-to-text) | AWS | ~$0.02/min |
| **Qwen 14B LLM** | Cognitive routing | GCP g2-standard-4 (L4 GPU) | $0.40/hr |
| **Polly Neural TTS** | Voice synthesis + visemes | AWS | ~$0.02/1k chars |
| **Sumerian Hosts** | Avatar renderer | Browser (Three.js) | Free |
| **IPC Bus (Miranda)** | Real-time sync, latency tracking | Local tmpfs (/dev/shm) | Free |

---


## Phase One Design Specs (Miranda's Brain — Complete)

Phase One of Miranda's cognitive architecture is now fully specced (requirements → design → tasks), covering two new Work Orders in addition to WO-1 through WO-5:

### WO-Memory: Memory Data Lake (`.kiro/specs/wo-memory-data-lake/`)

A bi-directional, local-first memory system built on three coordinated backends:
- **Neo4j** (rootless Podman) — knowledge graph of entities, conversations, mood states, relationships
- **Obsidian vault** — human-readable, bidirectionally-linked markdown notes for browsing history
- **DuckDB** — SQL-queryable event index for fast analytics (mood filtering, entity lookup)
- **Data lake** — immutable JSONL event log, source of truth for all conversation turns

All storage lives under `/mnt/NOBILITY_VAULT/.miranda/`, encrypted at rest, zero cloud transmission. Retrieval is bi-directional: every new user message triggers a graph + index query for relevant past context (entity overlap, temporal recency, mood continuity), which is injected into the LLM system prompt before inference. This is what lets Miranda reference "yesterday's training run" or "the quantization issue we debugged last week" instead of resetting context every session.

14 tasks, CAT 1-3 only (no CAT 4/5 in this spec), routed primarily to Nova Pro.

### WO-Conversational-Intelligence: Adaptive Conversation Layer (`.kiro/specs/wo-conversational-intelligence/`)

Moves Miranda from reactive Q&A to adaptive, anticipatory conversation:
- **Continuous mood tracking** — mood is a live vector updated mid-message, not a per-turn snapshot; drives real-time avatar ARB color transitions
- **Conversation state machine** — Opening/Deep Work/Debugging/Reflection/Casual states with micro-states, controlling response depth and tone
- **Anticipatory move generator** — proactively surfaces next-step suggestions above a 0.7 confidence threshold (CAT 4 — real correctness risk, since a wrong confident prediction is worse than none)
- **Interest model & curiosity engine** — Miranda tracks recurring topics/techniques and surfaces genuine questions about the user's own work (rate-limited to ≤1/hour)
- **Real-time knowledge updates** — corrections, framework mentions, and code style apply forward within the same session
- **Role/persona fluidity** — Research Partner, Rubber Duck, Peer Reviewer, Therapist, Brainstorm Co-Creator, auto-detected from conversational cues
- **Autonomy calibration interview** — Miranda interviews the user on acceptable autonomy per action category (file ops, spending/GPU provisioning, git, install/config); stores thresholds and periodically re-checks as a track record builds
- **Fixed autonomy floor (non-negotiable)** — destructive-at-scale, production-impacting, and high-blast-radius actions always require explicit confirmation regardless of calibration; this holds under every possible interview input
- **Partnership investment tracking** — tracks user goals, surfaces progress unprompted, filtered against a banned-pattern list so acknowledgment never uses dependency or guilt language
- **Mahogany Hall groundwork** — same memory/persona architecture supports sustained role-play and relationship continuity for the companionship project, using the same local-encrypted storage guarantees

11 tasks; only Task 3 (Anticipatory Move Generator) is CAT 4, everything else CAT 2-3.

Both specs pass full format validation (`validate_spec_format`) with zero errors.

## Deployment Checklist

- [ ] `.env` file filled in with all credentials
- [ ] AWS credentials verified with `aws sts get-caller-identity`
- [ ] GCP berylize-node is running and accessible (ping 34.26.220.164)
- [ ] Qwen llama-server responding on port 8000
- [ ] `npm run dev` started in client-apps/web
- [ ] Settings Panel ⚙️ shows all keys as "Connected" (green)
- [ ] Browser microphone working (test in browser console)
- [ ] THE VANITY UI renders canvas on the right

---

## Idle Auto-Stop (Cost Discipline)

The berylize-node instance will **auto-stop after 15 minutes of inactivity** to control GCP costs.

To keep it running during development:

```bash
# From your local machine, ping the instance every 5 minutes
watch -n 300 'curl -s http://34.26.220.164:8000/v1/models > /dev/null && echo "Instance alive"'
```

Or manually restart if stopped:

```bash
gcloud compute instances start berylize-node --zone=us-east1-c --project=posh-eden
```

---

## Troubleshooting

### "Connection refused" on port 8000

- Verify berylize-node is running: `gcloud compute instances list --project=posh-eden`
- Check if llama-server crashed: SSH and run `ps aux | grep llama-server`
- Restart the instance: `gcloud compute instances stop/start berylize-node ...`

### Polly returns empty visemes

- Check AWS Bedrock region is correct (should be us-east-1 or us-west-2)
- Verify Polly model access is enabled in AWS Console → Bedrock → Model access
- Ensure `SpeechMarkTypes: ['viseme']` is set in the Polly request

### Avatar not animating

- Open browser DevTools → Network tab, check if WebSocket to Polly is open
- Verify BlendshapeFrame messages are flowing on the Miranda IPC bus (check `/dev/shm/miranda_bus`)
- Check if Sumerian Hosts is loaded: `curl http://localhost:5173/index.html | grep sumerian`

### "VcpuLimitExceeded" on AWS (GPU quota issue)

- This means your AWS startup account doesn't have GPU quota yet
- For now, deploy on **t3.large (CPU)** and test the pipeline topology
- AWS typically approves GPU quota increases within 24-48 hours; check Support console

---

## Next Steps (Post-Alpha)

1. **Pipeline 1 validation:** Measure end-to-end latency (Transcribe → Bedrock → Polly → render)
2. **Pipeline 2 (Research):** Swap GCP Qwen for **parakeet.cpp** (Riva ASR on-device) + **SIMD kinematics** (52-channel blend shape solver)
3. **Pipeline 3 (GPU rendering):** Replace Sumerian Hosts with **WebGPU Gaussian-splat renderer** (WO-5)
4. **Quad-test:** Run Pipeline 1, 2, 3, and variants in parallel, score each against the **Instant Presence Standard**

---

## Versioning & Updates

Every commit to this repo updates this **HANDOFF.md** with:
- New services added / removed
- IP addresses or port changes
- Credentials rotation (if needed)
- Deployment checklist updates
- Cost tracking changes
- New troubleshooting entries

**Current commit:** `fb34b5f` (Aug 27, 2026)  
**Synced to:**
- GitHub: https://github.com/tyronne-os/new-toy
- Hugging Face (backup): https://huggingface.co/AIBRUH/miranda-engine

---

## Support

- **Slack:** #miranda-engine (Beryl Labs)
- **Issues:** Open a GitHub issue on this repo
- **Email:** contact@beryllabs.com

Happy streaming! 🎬✨
