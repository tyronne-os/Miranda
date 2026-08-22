"""
EVE ECC — ACE Controller
Instant Presence orchestrator: control plane always answers; data plane warms.
Live path: NVIDIA NIM chat (Nemotron) when NVIDIA_API_KEY is set.

Run:
  uvicorn app.main:app --host 127.0.0.1 --port 8100 --reload
"""

from __future__ import annotations

import asyncio
import os
import time
from enum import Enum
from typing import Any

from dotenv import load_dotenv
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from .nvidia_live import chat_completion, nvidia_configured

# Load repo-root .env then local overrides (never commit secrets)
_ROOT_ENV = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".env"))
load_dotenv(_ROOT_ENV)
load_dotenv()

APP_START = time.time()
HOST = os.getenv("ACE_HOST", "127.0.0.1")
# live if key present unless explicitly forced to mock
_force = os.getenv("ACE_MODE", "").strip().lower()
if _force in {"mock", "live"}:
    MODE = _force
else:
    MODE = "live" if nvidia_configured() else "mock"


class Stage(str, Enum):
    L0 = "L0"
    L1 = "L1"
    L2 = "L2"


NODES = [
    "mic",
    "presence",
    "syncer",
    "riva-asr",
    "nemotron",
    "riva-tts",
    "a2f",
    "animgraph",
    "omniverse",
]

CONTROL = {"mic", "presence", "syncer"}


class StageRequest(BaseModel):
    stage: Stage


class TalkRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=4000)
    promote: bool = True


class ControllerState:
    def __init__(self) -> None:
        self.stage = Stage.L0
        self.target_stage = Stage.L0
        self.warm_progress = 0.4
        self.clients: set[WebSocket] = set()
        self._lock = asyncio.Lock()
        self.last_talk: dict[str, Any] | None = None
        self.last_model: str | None = None
        self.talking = False

    def health_for(self, node_id: str) -> str:
        if node_id in CONTROL:
            return "hot" if self.talking else "ready"
        if MODE == "live" and node_id == "nemotron" and nvidia_configured():
            if self.stage in {Stage.L1, Stage.L2} or self.talking:
                return "hot"
            return "ready" if self.warm_progress > 0.35 else "warming"
        if self.stage == Stage.L0:
            return "warming" if self.warm_progress > 0.5 else "cold"
        if self.stage == Stage.L1:
            return "warming" if node_id == "omniverse" else "hot"
        return "hot"

    def snapshot(self) -> dict[str, Any]:
        t = int((time.time() - APP_START) * 1000)
        return {
            "type": "snapshot",
            "tMediaMs": t,
            "stage": self.stage.value,
            "targetStage": self.target_stage.value,
            "warmProgress": round(self.warm_progress, 3),
            "mode": MODE,
            "controlMs": 38,
            "nvidia": {
                "configured": nvidia_configured(),
                "model": self.last_model,
            },
            "lastTalk": self.last_talk,
            "nodes": {
                n: {
                    "id": n,
                    "health": self.health_for(n),
                    "latencyMs": 24 if n in CONTROL else (120 if n == "nemotron" else 80),
                    "load": 0.62 if self.health_for(n) == "hot" else 0.2,
                    "message": self.health_for(n),
                }
                for n in NODES
            },
            "event": {
                "kind": "system",
                "level": "ok",
                "message": f"ACE controller · {MODE} · {self.stage.value}",
            },
        }


state = ControllerState()

app = FastAPI(
    title="EVE ECC ACE Controller",
    version="0.2.0",
    description="Instant Presence stage bus + NVIDIA NIM live talk path",
)

# Dev-only CORS for local IDE. Do not widen casually in production.
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:4173",
        "http://localhost:4173",
    ],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health() -> dict[str, Any]:
    return {
        "ok": True,
        "mode": MODE,
        "stage": state.stage.value,
        "nvidia": nvidia_configured(),
    }


@app.get("/v1/status")
async def status() -> dict[str, Any]:
    return state.snapshot()


@app.post("/v1/stage")
async def set_stage(body: StageRequest) -> dict[str, Any]:
    async with state._lock:
        state.target_stage = body.stage
        state.stage = body.stage
        state.warm_progress = 0.45 if body.stage == Stage.L0 else 1.0
    snap = state.snapshot()
    await broadcast(snap)
    return snap


@app.post("/v1/talk")
async def talk(body: TalkRequest) -> dict[str, Any]:
    """Wire user text → NVIDIA Nemotron → EVE reply. Promotes toward L1."""
    text = body.text.strip()
    if not text:
        return {"ok": False, "error": "empty text", **state.snapshot()}

    async with state._lock:
        state.talking = True
        if body.promote and state.stage == Stage.L0:
            state.target_stage = Stage.L1
            state.stage = Stage.L1
            state.warm_progress = max(state.warm_progress, 0.85)

    await broadcast(state.snapshot())

    if MODE == "mock" and not nvidia_configured():
        reply = (
            "I'm with you on the Instant Presence path. "
            "Set NVIDIA_API_KEY in .env to open the live Nemotron channel."
        )
        result = {
            "ok": True,
            "reply": reply,
            "model": "mock-eve",
            "latencyMs": 12,
            "error": None,
        }
    else:
        result = await chat_completion(text)
        if not result.get("ok"):
            result = {
                "ok": False,
                "reply": (
                    "I heard you — the NVIDIA path returned an error. "
                    "Control plane is still live; check the key and model access."
                ),
                "model": result.get("model"),
                "latencyMs": result.get("latencyMs") or 0,
                "error": result.get("error"),
            }

    async with state._lock:
        state.talking = False
        state.last_model = result.get("model")
        state.last_talk = {
            "userText": text,
            "reply": result.get("reply") or "",
            "ok": bool(result.get("ok")),
            "model": result.get("model"),
            "latencyMs": result.get("latencyMs") or 0,
            "error": result.get("error"),
            "at": time.time(),
        }
        if result.get("ok"):
            state.warm_progress = max(state.warm_progress, 0.92)
            if state.stage == Stage.L0:
                state.stage = Stage.L1
                state.target_stage = Stage.L1

    snap = state.snapshot()
    snap.update(
        {
            "ok": bool(result.get("ok")),
            "reply": result.get("reply") or "",
            "model": result.get("model"),
            "latencyMs": result.get("latencyMs") or 0,
            "error": result.get("error"),
        }
    )
    await broadcast(snap)
    return snap


async def broadcast(message: dict[str, Any]) -> None:
    dead: list[WebSocket] = []
    for ws in list(state.clients):
        try:
            await ws.send_json(message)
        except Exception:
            dead.append(ws)
    for ws in dead:
        state.clients.discard(ws)


@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket) -> None:
    await ws.accept()
    state.clients.add(ws)
    await ws.send_json(state.snapshot())
    try:
        while True:
            msg = await ws.receive_json()
            mtype = msg.get("type")
            if mtype == "ping":
                await ws.send_json({"type": "pong", "t": time.time()})
            elif mtype == "stage":
                stage = msg.get("stage")
                if stage in Stage._value2member_map_:
                    async with state._lock:
                        state.stage = Stage(stage)
                        state.target_stage = Stage(stage)
                        state.warm_progress = 0.45 if stage == "L0" else 1.0
                    await broadcast(state.snapshot())
            elif mtype == "talk":
                text = str(msg.get("text") or "").strip()
                if text:
                    await talk(TalkRequest(text=text, promote=bool(msg.get("promote", True))))
    except WebSocketDisconnect:
        pass
    finally:
        state.clients.discard(ws)


@app.on_event("startup")
async def startup() -> None:
    async def warmer() -> None:
        while True:
            await asyncio.sleep(1.0)
            if state.stage == Stage.L0 and state.warm_progress < 0.7:
                state.warm_progress = min(0.7, state.warm_progress + 0.01)
            if MODE == "live" and nvidia_configured() and state.warm_progress < 0.55:
                state.warm_progress = min(0.55, state.warm_progress + 0.02)
            if state.clients:
                await broadcast(state.snapshot())

    asyncio.create_task(warmer())
