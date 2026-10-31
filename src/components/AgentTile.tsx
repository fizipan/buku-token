import { AGENT_LOGO, AGENT_MONOGRAM, modelLogo } from "@/lib/agents";
import type { AgentKey } from "@/lib/api";
import { Logo } from "./Logo";

/** Rounded tile in the agent color. `model` swaps in the model maker's logo (e.g. Kimi inside OpenCode). */
export function AgentTile({ agent, model, size = 24 }: { agent: AgentKey; model?: string; size?: number }) {
  const logo = (model ? modelLogo(model) : null) ?? AGENT_LOGO[agent];
  return (
    <span
      aria-hidden="true"
      className="inline-flex shrink-0 items-center justify-center rounded-[7px] text-[9px] font-semibold tracking-tight"
      style={{
        width: size,
        height: size,
        background: `color-mix(in oklab, var(--a-${agent}) 15%, var(--background))`,
        color: `var(--a-${agent})`,
      }}
    >
      {logo ? <Logo id={logo} size={Math.round(size * 0.58)} /> : AGENT_MONOGRAM[agent]}
    </span>
  );
}
