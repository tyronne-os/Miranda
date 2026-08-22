import type { NodeHealth, PresenceStage } from "@/lib/stageMachine/types";

export type BusLinkState = "offline" | "connecting" | "live" | "mock" | "error";

export interface AceNodeSnapshot {
    id: string;
    health: NodeHealth;
    latencyMs: number;
    load: number;
    message?: string;
}

export interface AceSnapshot {
    type?: string;
    tMediaMs?: number;
    stage: PresenceStage;
    targetStage?: PresenceStage;
    warmProgress: number;
    mode: string;
    controlMs?: number;
    nodes?: Record<string, AceNodeSnapshot>;
    event?: {
        kind?: string;
        level?: "info" | "ok" | "warn" | "error";
        message?: string;
    };
    nvidia?: {
        configured: boolean;
        model?: string | null;
    };
    lastTalk?: {
        userText?: string;
        reply?: string;
        ok?: boolean;
        model?: string | null;
        latencyMs?: number;
        error?: string | null;
        at?: number;
    } | null;
}

export interface TalkResult {
    ok: boolean;
    reply: string;
    model?: string | null;
    latencyMs?: number;
    error?: string | null;
    stage?: PresenceStage;
    mode?: string;
}

/** Pipe 3 Phoneme-Direct frame — mouth truth derived from text, not audio. */
export interface VisemeFrame {
    tMediaMs: number;
    viseme: string;
    energy: number;
    weights: Record<string, number>;
}

type SnapshotHandler = (snap: AceSnapshot) => void;
type LinkHandler = (state: BusLinkState, detail?: string) => void;
type VisemeHandler = (frames: VisemeFrame[], durationMs: number, source?: string) => void;

const DEFAULT_HTTP = "http://127.0.0.1:8100";
const DEFAULT_WS = "ws://127.0.0.1:8100/ws";

function httpBase() {
    return (import.meta.env.VITE_ACE_HTTP_URL || DEFAULT_HTTP).replace(/\/$/, "");
}

function wsUrl() {
    return import.meta.env.VITE_ACE_WS_URL || DEFAULT_WS;
}

class AceControllerClient {
    private ws: WebSocket | null = null;
    private reconnectTimer: number | null = null;
    private intentionalClose = false;
    private onSnapshot: SnapshotHandler | null = null;
    private onLink: LinkHandler | null = null;
    private onVisemes: VisemeHandler | null = null;
    private link: BusLinkState = "offline";

    get linkState() {
        return this.link;
    }

    configure(handlers: {
        onSnapshot: SnapshotHandler;
        onLink: LinkHandler;
        onVisemes?: VisemeHandler;
    }) {
        this.onSnapshot = handlers.onSnapshot;
        this.onLink = handlers.onLink;
        this.onVisemes = handlers.onVisemes ?? null;
    }

    private setLink(state: BusLinkState, detail?: string) {
        this.link = state;
        this.onLink?.(state, detail);
    }

    start() {
        this.intentionalClose = false;
        void this.probeAndConnect();
    }

    stop() {
        this.intentionalClose = true;
        if (this.reconnectTimer != null) {
            window.clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
        this.ws?.close();
        this.ws = null;
        this.setLink("offline", "bus stopped");
    }

    private scheduleReconnect() {
        if (this.intentionalClose) return;
        if (this.reconnectTimer != null) return;
        this.reconnectTimer = window.setTimeout(() => {
            this.reconnectTimer = null;
            void this.probeAndConnect();
        }, 1500);
    }

    private async probeAndConnect() {
        this.setLink("connecting", "probing ACE controller");
        try {
            const res = await fetch(`${httpBase()}/health`, {
                method: "GET",
                cache: "no-store",
            });
            if (!res.ok) throw new Error(`health ${res.status}`);
            const body = (await res.json()) as { mode?: string; stage?: string };
            const mode = body.mode || "unknown";
            this.connectWs(mode);
        } catch (err) {
            const msg = err instanceof Error ? err.message : "unreachable";
            this.setLink("error", msg);
            this.scheduleReconnect();
        }
    }

    private connectWs(mode: string) {
        try {
            const ws = new WebSocket(wsUrl());
            this.ws = ws;

            ws.onopen = () => {
                const link: BusLinkState =
                    mode.includes("live") || mode === "live" ? "live" : mode.includes("mock") ? "mock" : "live";
                this.setLink(link, `ws open · ${mode}`);
                ws.send(JSON.stringify({ type: "ping" }));
            };

            ws.onmessage = (ev) => {
                try {
                    const data = JSON.parse(String(ev.data)) as AceSnapshot & {
                        type?: string;
                        frames?: VisemeFrame[];
                        durationMs?: number;
                        source?: string;
                    };
                    if (data.type === "pong") return;
                    if (data.type === "visemes" && Array.isArray(data.frames)) {
                        this.onVisemes?.(data.frames, data.durationMs ?? 0, data.source);
                        return;
                    }
                    if (data.type === "snapshot" || data.stage) {
                        this.onSnapshot?.(data);
                        if (data.mode) {
                            const link: BusLinkState =
                                data.mode.includes("live") ? "live" : data.mode.includes("mock") ? "mock" : this.link;
                            if (link !== this.link && (link === "live" || link === "mock")) {
                                this.setLink(link, `mode ${data.mode}`);
                            }
                        }
                    }
                } catch {
                    /* ignore malformed */
                }
            };

            ws.onerror = () => {
                this.setLink("error", "websocket error");
            };

            ws.onclose = () => {
                this.ws = null;
                if (!this.intentionalClose) {
                    this.setLink("offline", "ws closed");
                    this.scheduleReconnect();
                }
            };
        } catch (err) {
            const msg = err instanceof Error ? err.message : "ws failed";
            this.setLink("error", msg);
            this.scheduleReconnect();
        }
    }

    async setStage(stage: PresenceStage): Promise<AceSnapshot | null> {
        try {
            const res = await fetch(`${httpBase()}/v1/stage`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ stage }),
            });
            if (!res.ok) return null;
            const snap = (await res.json()) as AceSnapshot;
            this.onSnapshot?.(snap);
            return snap;
        } catch {
            // fall back to ws command
            this.ws?.send(JSON.stringify({ type: "stage", stage }));
            return null;
        }
    }

    async talk(text: string, opts?: { promote?: boolean }): Promise<TalkResult> {
        const payload = {
            text,
            promote: opts?.promote ?? true,
        };
        try {
            const res = await fetch(`${httpBase()}/v1/talk`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });
            const data = (await res.json()) as TalkResult & AceSnapshot;
            if (data.stage) this.onSnapshot?.(data as AceSnapshot);
            return {
                ok: Boolean(data.ok),
                reply: data.reply || "",
                model: data.model,
                latencyMs: data.latencyMs,
                error: data.error,
                stage: data.stage,
                mode: data.mode,
            };
        } catch (err) {
            return {
                ok: false,
                reply: "",
                error: err instanceof Error ? err.message : "talk failed",
            };
        }
    }

    async status(): Promise<AceSnapshot | null> {
        try {
            const res = await fetch(`${httpBase()}/v1/status`, { cache: "no-store" });
            if (!res.ok) return null;
            return (await res.json()) as AceSnapshot;
        } catch {
            return null;
        }
    }
}

export const aceController = new AceControllerClient();
