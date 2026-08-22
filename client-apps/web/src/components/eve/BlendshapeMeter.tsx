import { topBlendshapes } from "@/lib/ace/arkit";
import type { BlendshapeFrame } from "@/lib/stageMachine/types";

export function BlendshapeMeter({ frame }: { frame: BlendshapeFrame }) {
  const top = topBlendshapes(frame, 10);

  return (
    <div className="blend-meter">
      <div className="blend-meter-head">
        <strong>ARKit 52</strong>
        <span className="mono faint">viseme {frame.viseme}</span>
      </div>
      <div className="blend-list">
        {top.map((row) => (
          <div key={row.name} className="blend-row">
            <span className="blend-name">{row.name}</span>
            <div className="blend-track">
              <div className="blend-fill" style={{ width: `${Math.round(row.value * 100)}%` }} />
            </div>
            <span className="blend-val mono">{row.value.toFixed(2)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
