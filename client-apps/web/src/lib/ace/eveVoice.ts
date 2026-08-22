/**
 * EVE voice I/O helpers.
 * Mic transcript via Web Speech API when available.
 * Reply playback via speechSynthesis until Riva/A2F audio plane is live.
 * No drawn avatars — audio + energy only.
 */

export function canListen(): boolean {
    return typeof window !== "undefined" &&
        Boolean((window as unknown as { SpeechRecognition?: unknown; webkitSpeechRecognition?: unknown }).SpeechRecognition ||
            (window as unknown as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition);
}

export function canSpeak(): boolean {
    return typeof window !== "undefined" && "speechSynthesis" in window;
}

type ListenHandlers = {
    onPartial?: (text: string) => void;
    onFinal?: (text: string) => void;
    onError?: (message: string) => void;
    onEnd?: () => void;
};

/** Web Speech API event shims — lib.dom omits these in some TS configs. */
interface SpeechRecognitionResultEventLike {
    resultIndex: number;
    results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }>;
}
interface SpeechRecognitionErrorEventLike {
    error?: string;
}
interface SpeechRecognitionLike {
    lang: string;
    interimResults: boolean;
    continuous: boolean;
    onresult: ((event: SpeechRecognitionResultEventLike) => void) | null;
    onerror: ((event: SpeechRecognitionErrorEventLike) => void) | null;
    onend: (() => void) | null;
    start: () => void;
    stop: () => void;
    abort?: () => void;
}

export function listenOnce(handlers: ListenHandlers = {}): { stop: () => void } | null {
    const SR =
        (window as unknown as { SpeechRecognition?: new () => SpeechRecognitionLike }).SpeechRecognition ||
        (window as unknown as { webkitSpeechRecognition?: new () => SpeechRecognitionLike }).webkitSpeechRecognition;

    if (!SR) {
        handlers.onError?.("Speech recognition unavailable in this browser");
        return null;
    }

    const rec = new SR();
    rec.lang = "en-US";
    rec.interimResults = true;
    rec.continuous = false;
    let finalText = "";

    rec.onresult = (event: SpeechRecognitionResultEventLike) => {
        let interim = "";
        for (let i = event.resultIndex; i < event.results.length; i += 1) {
            const piece = event.results[i]?.[0]?.transcript || "";
            if (event.results[i]?.isFinal) finalText += `${piece} `;
            else interim += piece;
        }
        const partial = (finalText + interim).trim();
        if (partial) handlers.onPartial?.(partial);
    };

    rec.onerror = (event: SpeechRecognitionErrorEventLike) => {
        handlers.onError?.(event.error || "listen error");
    };

    rec.onend = () => {
        const text = finalText.trim();
        if (text) handlers.onFinal?.(text);
        handlers.onEnd?.();
    };

    try {
        rec.start();
    } catch (err) {
        handlers.onError?.(err instanceof Error ? err.message : "listen start failed");
        return null;
    }

    return {
        stop: () => {
            try {
                rec.stop();
            } catch {
                /* ignore */
            }
        },
    };
}

export function speakText(
    text: string,
    opts?: {
        onStart?: () => void;
        onEnd?: () => void;
        onBoundary?: (charIndex: number) => void;
        rate?: number;
        pitch?: number;
    },
): { cancel: () => void } {
    if (!canSpeak() || !text.trim()) {
        opts?.onEnd?.();
        return { cancel: () => undefined };
    }

    window.speechSynthesis.cancel();
    const utter = new SpeechSynthesisUtterance(text.trim());
    utter.rate = opts?.rate ?? 1.02;
    utter.pitch = opts?.pitch ?? 1.05;
    utter.lang = "en-US";

    // Prefer a natural female-presenting voice when the OS provides one.
    const voices = window.speechSynthesis.getVoices();
    const preferred =
        voices.find((v) => /aria|jenny|sara|female|zira|google us english/i.test(v.name)) ||
        voices.find((v) => v.lang?.toLowerCase().startsWith("en"));
    if (preferred) utter.voice = preferred;

    utter.onstart = () => opts?.onStart?.();
    utter.onend = () => opts?.onEnd?.();
    utter.onerror = () => opts?.onEnd?.();
    utter.onboundary = (ev) => {
        if (typeof ev.charIndex === "number") opts?.onBoundary?.(ev.charIndex);
    };

    window.speechSynthesis.speak(utter);

    return {
        cancel: () => {
            window.speechSynthesis.cancel();
        },
    };
}

// Warm voices list (Chrome loads async)
if (typeof window !== "undefined" && "speechSynthesis" in window) {
    window.speechSynthesis.getVoices();
    window.speechSynthesis.onvoiceschanged = () => {
        window.speechSynthesis.getVoices();
    };
}
