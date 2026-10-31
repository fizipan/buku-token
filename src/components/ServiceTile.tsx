import { Logo } from "./Logo";

export type ServiceKey = "openrouter" | "antigravity";

/**
 * Rounded tile for what sits beside the agents: neutral for OpenRouter, Google blue (`--svc-ag`,
 * the same tint pattern as an agent tile) for Antigravity.
 */
export function ServiceTile({ service, size = 24 }: { service: ServiceKey; size?: number }) {
  const blue = service === "antigravity";
  return (
    <span
      aria-hidden="true"
      className={`inline-flex shrink-0 items-center justify-center rounded-[7px] ${blue ? "" : "bg-muted text-muted-foreground"}`}
      style={{
        width: size,
        height: size,
        ...(blue && { background: "color-mix(in oklab, var(--svc-ag) 15%, var(--background))", color: "var(--svc-ag)" }),
      }}
    >
      <Logo id={service} size={Math.round(size * 0.58)} />
    </span>
  );
}
