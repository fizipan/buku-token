import { describe, expect, it } from "vitest";
import { AGENT_LOGO, AGENT_NAME, STACK_ORDER, modelLabel, modelLogo } from "./agents";

describe("agents", () => {
  it("stack order matches the Rust byAgent order", () => {
    expect(STACK_ORDER).toEqual(["cc", "cx", "oc", "hm", "cr"]);
  });
  it("names and logos", () => {
    expect(AGENT_NAME.cc).toBe("Claude Code");
    expect(AGENT_NAME.oc).toBe("OpenCode");
    expect(AGENT_LOGO.cc).toBe("claude");
    expect(AGENT_LOGO.cx).toBe("openai");
    expect(AGENT_LOGO.oc).toBeNull();
    expect(AGENT_LOGO.hm).toBe("nousresearch");
    expect(AGENT_NAME.cr).toBe("Cursor");
    expect(AGENT_LOGO.cr).toBe("cursor");
  });
  it("modelLogo maps model families, case-insensitive", () => {
    expect(modelLogo("claude-opus-5-5")).toBe("claude");
    expect(modelLogo("Claude-Sonnet-5")).toBe("claude");
    expect(modelLogo("gpt-5.5")).toBe("openai");
    expect(modelLogo("o3-mini")).toBe("openai");
    expect(modelLogo("kimi-k2")).toBe("kimi");
    expect(modelLogo("MiniMax-M2")).toBe("minimax");
    expect(modelLogo("big-pickle")).toBeNull();
  });
  it("modelLogo maps Cursor models and effort-suffixed ids", () => {
    expect(modelLogo("composer-2.5")).toBe("cursor");
    expect(modelLogo("composer-2.5-fast")).toBe("cursor");
    expect(modelLogo("default")).toBe("cursor");
    expect(modelLogo("auto")).toBe("cursor");
    expect(modelLogo("claude-opus-5-5-medium")).toBe("claude");
    expect(modelLogo("gpt-5.5-high")).toBe("openai");
  });
});

describe("modelLabel", () => {
  it("prettifies Claude ids and leaves others alone", () => {
    expect(modelLabel("claude-opus-5-5")).toBe("Opus 5.5");
    expect(modelLabel("claude-sonnet-4-20250514")).toBe("Sonnet 4");
    expect(modelLabel("claude-3-5-haiku-20241022")).toBe("Haiku 3.5");
    expect(modelLabel("gpt-5.2-codex")).toBe("GPT-5.2 Codex");
    expect(modelLabel("gpt-5.5")).toBe("GPT-5.5");
    expect(modelLabel("gpt-5.1-codex-max")).toBe("GPT-5.1 Codex Max");
    expect(modelLabel("kimi-k2")).toBe("kimi-k2");
    expect(modelLabel("default")).toBe("Auto");
    expect(modelLabel("auto")).toBe("Auto");
    expect(modelLabel("composer-2.5")).toBe("Composer 2.5");
    expect(modelLabel("composer-2.5-fast")).toBe("Composer 2.5 Fast");
    expect(modelLabel("claude-opus-5-5-medium")).toBe("Opus 5.5");
    expect(modelLabel("claude-sonnet-4-20250514-high")).toBe("Sonnet 4");
    expect(modelLabel("gpt-5.5-high")).toBe("GPT-5.5");
    expect(modelLabel("gpt-5.1-codex-max")).toBe("GPT-5.1 Codex Max");
    expect(modelLabel("kimi-k2-low")).toBe("kimi-k2-low");
    expect(modelLabel("gpt-5.1-codex-max-high")).toBe("GPT-5.1 Codex Max");
    expect(modelLabel("big-pickle")).toBe("big-pickle");
  });
});
