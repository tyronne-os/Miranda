"""
NVIDIA live path for EVE ECC ACE controller.

Uses build.nvidia.com / integrate.api.nvidia.com OpenAI-compatible chat.
Never logs or returns the API key.
"""

from __future__ import annotations

import os
import time
from typing import Any

import httpx

NVIDIA_BASE = os.getenv("NVIDIA_BASE_URL", "https://integrate.api.nvidia.com/v1").rstrip("/")
NEMOTRON_MODEL = os.getenv(
    "NEMOTRON_MODEL",
    "nvidia/llama-3.1-nemotron-70b-instruct",
)
# Smaller fallback if 70B is unavailable on the key
NEMOTRON_FALLBACK = os.getenv(
    "NEMOTRON_FALLBACK_MODEL",
    "meta/llama-3.1-8b-instruct",
)

EVE_SYSTEM = """You are EVE — Executive Presence for The Cerebral Project (Beryl Labs).
You are warm, precise, human, and alive. Short replies (1-3 sentences) unless asked for depth.
You speak with calm confidence and natural energy — never robotic, never corporate filler.
You are the live presence on the ACE Instant Presence pipeline.
Do not mention being an AI unless asked. Do not invent visual placeholders or claim you see cameras you do not have.
"""


def api_key() -> str | None:
    key = (
        os.getenv("NVIDIA_API_KEY")
        or os.getenv("NGC_API_KEY")
        or os.getenv("NVAPI_KEY")
        or ""
    ).strip()
    return key or None


def nvidia_configured() -> bool:
    return api_key() is not None


async def chat_completion(user_text: str, *, model: str | None = None) -> dict[str, Any]:
    """Call NVIDIA NIM chat. Returns {ok, reply, model, latencyMs, error?}."""
    key = api_key()
    if not key:
        return {
            "ok": False,
            "reply": "",
            "model": None,
            "latencyMs": 0,
            "error": "NVIDIA_API_KEY not set",
        }

    primary = model or os.getenv("NEMOTRON_BASE_URL") and None
    # NEMOTRON_BASE_URL historically meant base URL; model comes from NEMOTRON_MODEL
    models = [m for m in [model, NEMOTRON_MODEL, NEMOTRON_FALLBACK] if m]
    # dedupe preserve order
    seen: set[str] = set()
    ordered: list[str] = []
    for m in models:
        if m not in seen:
            seen.add(m)
            ordered.append(m)

    headers = {
        "Authorization": f"Bearer {key}",
        "Content-Type": "application/json",
        "Accept": "application/json",
    }

    last_err = "no model attempted"
    t0 = time.perf_counter()

    async with httpx.AsyncClient(timeout=httpx.Timeout(60.0, connect=10.0)) as client:
        for mid in ordered:
            payload = {
                "model": mid,
                "messages": [
                    {"role": "system", "content": EVE_SYSTEM},
                    {"role": "user", "content": user_text},
                ],
                "temperature": 0.7,
                "top_p": 0.9,
                "max_tokens": 220,
                "stream": False,
            }
            try:
                res = await client.post(
                    f"{NVIDIA_BASE}/chat/completions",
                    headers=headers,
                    json=payload,
                )
                latency = int((time.perf_counter() - t0) * 1000)
                if res.status_code >= 400:
                    last_err = f"{mid}: HTTP {res.status_code} {res.text[:240]}"
                    continue
                data = res.json()
                choice = (data.get("choices") or [{}])[0]
                message = choice.get("message") or {}
                reply = (message.get("content") or "").strip()
                if not reply:
                    last_err = f"{mid}: empty content"
                    continue
                return {
                    "ok": True,
                    "reply": reply,
                    "model": data.get("model") or mid,
                    "latencyMs": latency,
                    "usage": data.get("usage"),
                }
            except Exception as exc:  # noqa: BLE001 — surface to bus
                last_err = f"{mid}: {exc}"
                continue

    return {
        "ok": False,
        "reply": "",
        "model": None,
        "latencyMs": int((time.perf_counter() - t0) * 1000),
        "error": last_err,
    }
