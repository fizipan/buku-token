import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const listen = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: unknown[]) => listen(...a) }));

import * as api from "./api";
import type {
  HistoryView, ModelsView, Overview, ProjectsView, ScanStatus, SessionPage, Settings,
} from "./api";
import history from "../test/fixtures/history-view.json";
import models from "../test/fixtures/models-view.json";
import overviewQuota from "../test/fixtures/overview-quota.json";
import projects from "../test/fixtures/projects-view.json";
import scan from "../test/fixtures/scan-status.json";
import sessions from "../test/fixtures/session-page.json";
import settings from "../test/fixtures/settings.json";

const REQ: api.ShareRequest = { period: "30d", agent: null, hideCost: false, dark: true };

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
});

describe("wrappers call the right command with the right argument names", () => {
  it.each([
    ["getOverview", () => api.getOverview(), "get_overview", undefined],
    ["getSessions", () => api.getSessions({ days: 3, agent: "cc" }), "get_sessions", { filter: { days: 3, agent: "cc" } }],
    ["getModels", () => api.getModels("7d"), "get_models", { period: "7d" }],
    ["getProjects", () => api.getProjects("30d"), "get_projects", { period: "30d" }],
    ["getHistory", () => api.getHistory(14), "get_history", { days: 14 }],
    ["refresh", () => api.refresh(), "refresh", undefined],
    ["getSettings", () => api.getSettings(), "get_settings", undefined],
    ["saveSettings", () => api.saveSettings({ lastTab: "model" }), "save_settings", { settings: { lastTab: "model" } }],
    ["updatePricing", () => api.updatePricing(), "update_pricing", undefined],
    ["resizePopover", () => api.resizePopover(540), "resize_popover", { height: 540 }],
    ["openSettings", () => api.openSettings(), "open_settings", undefined],
    ["scanStatus", () => api.scanStatus(), "scan_status", undefined],
    ["openrouterSessionStatus", () => api.openrouterSessionStatus(), "openrouter_session_status", undefined],
    ["connectOpenRouter", () => api.connectOpenRouter("sk-or-v1-test"), "connect_openrouter", { key: "sk-or-v1-test" }],
    ["disconnectOpenRouter", () => api.disconnectOpenRouter(), "disconnect_openrouter", undefined],
    ["sharePreview", () => api.sharePreview(REQ), "share_preview", { request: REQ }],
    ["shareCopy", () => api.shareCopy(REQ), "share_copy", { request: REQ }],
    ["shareSave", () => api.shareSave(REQ), "share_save", { request: REQ }],
  ])("%s", async (_n, call, cmd, args) => {
    invoke.mockResolvedValue("ok");
    await expect(call()).resolves.toBe("ok");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0]).toEqual(args === undefined ? [cmd] : [cmd, args]);
  });

  it("rejections surface as the string from Rust", async () => {
    invoke.mockRejectedValue("boom");
    await expect(api.getOverview()).rejects.toBe("boom");
  });
});

describe("events", () => {
  it("onScanProgress unwraps the payload and returns the unlisten fn", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);
    const cb = vi.fn();
    const off = await api.onScanProgress(cb);
    expect(listen.mock.calls[0][0]).toBe("scan-progress");
    listen.mock.calls[0][1]({ payload: { done: 1, total: 4 } });
    expect(cb).toHaveBeenCalledWith({ done: 1, total: 4 });
    expect(off).toBe(unlisten);
  });
  it("onDataUpdated fires the callback without arguments", async () => {
    listen.mockResolvedValue(vi.fn());
    const cb = vi.fn();
    await api.onDataUpdated(cb);
    expect(listen.mock.calls[0][0]).toBe("data-updated");
    listen.mock.calls[0][1]({ payload: null });
    expect(cb).toHaveBeenCalledWith();
  });
});

// Fixtures are written by the Rust side (src-tauri/src/ts_fixtures.rs). Assigning them to the TS
// interfaces makes `tsc --noEmit` fail when a field is renamed or retyped on either side.
describe("Rust JSON matches the TS types", () => {
  it("Overview", () => {
    const q: Overview = overviewQuota as Overview;
    expect(q.today.byAgent).toEqual([12.5, 2.75, 0, 0, 0]);
    expect(q.today.unpricedByAgent).toEqual([0, 0, 1200, 0, 0]);
    expect(q.today.hours).toHaveLength(18);
    expect(q.burn?.points[1]).toEqual([60, 24.5]);
    expect(q.quotas[0].binding?.id).toBe("five_hour");
    expect(q.rtk?.savedTokens).toBe(98_000);
    expect(typeof q.updatedAt).toBe("string");
  });
  it("SessionPage", () => {
    const p: SessionPage = sessions as SessionPage;
    expect(p.hasMore).toBe(true);
    expect(p.days[0].sessions[1].costUsd).toBeNull();
    expect(p.days[0].sessions[0].project.displayPath).toBe("buku-token");
    expect(p.projects[0].path).toContain("buku-token");
  });
  it("ModelsView", () => {
    const m: ModelsView = models as ModelsView;
    expect(m.rows[1].costUsd).toBeNull();
    expect(m.rows[1].share).toBeNull();
    expect(m.tokenMix.cost.cacheRead).toBe(20);
    expect(m.cacheInsight?.costWithoutCache).toBe(40);
  });
  it("ProjectsView", () => {
    const p: ProjectsView = projects as ProjectsView;
    expect(p.rows[1].folded).toBe(3);
    expect(p.insight?.prevShare).toBe(35);
  });
  it("HistoryView", () => {
    const h: HistoryView = history as HistoryView;
    expect(h.days[0].byAgent).toHaveLength(5);
    expect(h.plans[0].cycle.endExclusive).toBe("2026-11-01");
    expect(h.plans[0].projectedMultiple).toBe(4.34);
  });
  it("Settings and ScanStatus", () => {
    const s: Settings = settings as Settings;
    expect(s.enabledAgents.oc).toBe(false);
    expect(s.plans[0].renewalDay).toBe(5);
    const st: ScanStatus = scan as ScanStatus;
    expect(st.sources[0].agent).toBe("cc");
    expect(st.lastScanAt).not.toBeNull();
  });
});
