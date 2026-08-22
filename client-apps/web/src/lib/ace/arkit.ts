import { ARKIT_CHANNELS, type BlendshapeFrame } from "@/lib/stageMachine/types";

const VISEMES = ["sil", "PP", "FF", "TH", "DD", "kk", "CH", "SS", "nn", "RR", "aa", "E", "ih", "oh", "ou"] as const;

function clamp01(n: number) {
  return Math.min(1, Math.max(0, n));
}

/** Procedural ARKit frame — stands in for A2F-3D until live NIM is wired. */
export function synthesizeArkitFrame(
  tMediaMs: number,
  opts: { talking: boolean; energy: number; stageLive: boolean },
): BlendshapeFrame {
  const t = tMediaMs / 1000;
  const energy = clamp01(opts.energy);
  const talk = opts.talking && opts.stageLive ? 1 : 0;

  const breath = (Math.sin(t * 1.2) + 1) / 2;
  const micro = (Math.sin(t * 0.35) + 1) / 2;
  const jaw = talk * (0.12 + energy * 0.45 * (0.55 + 0.45 * Math.sin(t * 9.5)));
  const smile = 0.08 + micro * 0.06 + talk * energy * 0.12;
  const blinkCycle = t % 4.2;
  const blink = blinkCycle > 4.0 ? clamp01((blinkCycle - 4.0) / 0.08) : blinkCycle > 3.92 ? 1 : 0;

  const weights: Record<string, number> = {};
  for (const ch of ARKIT_CHANNELS) {
    weights[ch] = 0;
  }

  weights.jawOpen = jaw;
  weights.mouthClose = talk * (1 - energy) * 0.15;
  weights.mouthSmileLeft = smile;
  weights.mouthSmileRight = smile * 0.98;
  weights.mouthLeft = talk * 0.04 * Math.sin(t * 3.1);
  weights.mouthRight = talk * 0.04 * Math.cos(t * 2.7);
  weights.mouthFunnel = talk * energy * 0.18 * Math.max(0, Math.sin(t * 7.2));
  weights.mouthPucker = talk * energy * 0.1 * Math.max(0, Math.sin(t * 5.1 + 1));
  weights.browInnerUp = 0.05 + micro * 0.08 + talk * 0.04;
  weights.browOuterUpLeft = 0.03 + micro * 0.04;
  weights.browOuterUpRight = 0.03 + micro * 0.05;
  weights.eyeBlinkLeft = blink;
  weights.eyeBlinkRight = blink * 0.96;
  weights.eyeLookInLeft = 0.04 * Math.sin(t * 0.5);
  weights.eyeLookOutRight = 0.04 * Math.sin(t * 0.5);
  weights.cheekSquintLeft = smile * 0.25;
  weights.cheekSquintRight = smile * 0.22;
  weights.noseSneerLeft = talk * energy * 0.03;
  weights.mouthUpperUpLeft = talk * jaw * 0.2;
  weights.mouthUpperUpRight = talk * jaw * 0.18;
  weights.mouthLowerDownLeft = talk * jaw * 0.25;
  weights.mouthLowerDownRight = talk * jaw * 0.22;

  // Idle breath shoulders into cheek puff subtly
  weights.cheekPuff = breath * 0.03 * (1 - talk);

  const viseme =
    !talk
      ? "sil"
      : VISEMES[Math.floor(((Math.sin(t * 6.5) + 1) / 2) * (VISEMES.length - 1))]!;

  return {
    tMediaMs,
    weights,
    energy: talk ? energy : breath * 0.15,
    viseme,
  };
}

export function topBlendshapes(
  frame: BlendshapeFrame,
  n = 8,
): Array<{ name: string; value: number }> {
  return Object.entries(frame.weights)
    .map(([name, value]) => ({ name, value }))
    .sort((a, b) => b.value - a.value)
    .slice(0, n);
}
