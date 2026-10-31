import type { AgentKey } from "./api";

export type LogoId = "claude" | "openai" | "kimi" | "minimax" | "nousresearch" | "cursor" | "openrouter" | "antigravity";

/** Same order as every `byAgent` array from Rust. */
export const STACK_ORDER: readonly AgentKey[] = ["cc", "cx", "oc", "hm", "cr"];

export const AGENT_NAME: Record<AgentKey, string> = {
  cc: "Claude Code",
  cx: "Codex",
  oc: "OpenCode",
  hm: "Hermes",
  cr: "Cursor",
};

/** `null` = no logo, the tile shows a monogram instead. */
export const AGENT_LOGO: Record<AgentKey, LogoId | null> = {
  cc: "claude",
  cx: "openai",
  oc: null,
  hm: "nousresearch",
  cr: "cursor",
};

export const AGENT_MONOGRAM: Record<AgentKey, string> = { cc: "CC", cx: "CX", oc: "OC", hm: "HM", cr: "CR" };

/** Logo of the model's maker, or null when unknown. */
export function modelLogo(model: string): LogoId | null {
  const m = model.toLowerCase();
  if (m.startsWith("composer") || m === "default" || m === "auto") return "cursor";
  if (m.startsWith("claude")) return "claude";
  if (m.startsWith("gpt") || /^o\d/.test(m)) return "openai";
  if (m.startsWith("kimi")) return "kimi";
  if (m.startsWith("minimax")) return "minimax";
  return null;
}

const CLAUDE_NEW = /^claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?$/;
const CLAUDE_OLD = /^claude-(\d+)(?:-(\d{1,2}))?-(opus|sonnet|haiku)(?:-\d{8})?$/;

const GPT = /^gpt-(\d+(?:\.\d+)?)((?:-[a-z][a-z0-9]*)*)$/i;

const EFFORT = /-(?:low|medium|high|max)$/;

/**
 * "claude-opus-5-5" -> "Opus 5.5", "gpt-5.2-codex" -> "GPT-5.2 Codex", "default" -> "Auto".
 * Cursor's effort suffix (-low/-medium/-high/-max) is dropped; other model ids are shown as they are.
 */
export function modelLabel(id: string): string {
  if (id === "default" || id === "auto") return "Auto";
  // "gpt-5.1-codex-max" is a model name, not an effort level, so that one keeps its suffix.
  const model = id.endsWith("codex-max") ? id : id.replace(EFFORT, "");
  if (/^composer-\d/.test(id)) return id.split("-").map((w) => `${w[0].toUpperCase()}${w.slice(1)}`).join(" ");
  const g = GPT.exec(model);
  if (g) {
    const rest = g[2].split("-").filter(Boolean).map((w) => `${w[0].toUpperCase()}${w.slice(1)}`);
    return [`GPT-${g[1]}`, ...rest].join(" ");
  }
  const m = CLAUDE_NEW.exec(model);
  const [family, major, minor] = m ? [m[1], m[2], m[3]] : (() => {
    const o = CLAUDE_OLD.exec(model);
    return o ? [o[3], o[1], o[2]] : [];
  })();
  if (!family) return id;
  return `${family[0].toUpperCase()}${family.slice(1)} ${major}${minor ? `.${minor}` : ""}`;
}
