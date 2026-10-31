import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import settingsFixture from "../test/fixtures/settings.json";
import statusFixture from "../test/fixtures/scan-status.json";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));
const win = vi.hoisted(() => ({ focus: null as null | ((e: { payload: boolean }) => void), unlisten: null as null | ReturnType<typeof vi.fn> }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: async (cb: (e: { payload: boolean }) => void) => {
      win.focus = cb;
      win.unlisten = vi.fn();
      return win.unlisten;
    },
  }),
}));
const auto = vi.hoisted(() => ({ isEnabled: vi.fn(), enable: vi.fn(), disable: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => auto);

import { setTheme } from "../lib/theme";
import { Settings } from "./Settings";

const status = {
  ...statusFixture,
  skipped: 3,
  sources: [
    { agent: "cc", path: "/Users/dev/.claude/projects", found: true },
    { agent: "cx", path: "/Users/dev/.codex/sessions", found: false },
    { agent: "cx", path: "/Users/dev/.codex/archived_sessions", found: false },
    { agent: "oc", path: "/Users/dev/.local/share/opencode/opencode.db", found: true },
    { agent: "hm", path: "/Users/dev/.hermes/state.db", found: true },
    { agent: "cr", path: "/Users/dev/.cursor/projects", found: false },
  ],
  activitySources: [
    { name: "Antigravity", path: "/Users/dev/.gemini/antigravity/conversations", found: true },
    { name: "Antigravity", path: "/Users/dev/.gemini/antigravity-ide/conversations", found: false },
    { name: "Antigravity", path: "/Users/dev/.gemini/antigravity-cli/conversation_summaries.db", found: true },
  ],
};

let backend: Record<string, unknown>;

beforeEach(() => {
  resetTauri();
  win.focus = null;
  auto.isEnabled.mockReset().mockResolvedValue(false);
  auto.enable.mockReset().mockResolvedValue(undefined);
  auto.disable.mockReset().mockResolvedValue(undefined);
  backend = { ...settingsFixture, onboarded: true };
  commands.get_settings = () => backend;
  commands.scan_status = () => status;
  document.documentElement.classList.remove("dark");
  setTheme("system");
  commands.save_settings = (a) => (backend = { ...backend, ...(a as { settings: object }).settings });
  commands.update_pricing = () => "2026-11-02";
});

const saved = () => invokeMock.mock.calls.filter((c) => c[0] === "save_settings").map((c) => (c[1] as { settings: any }).settings);

async function ready() {
  render(<Settings />);
  await screen.findByRole("heading", { name: "Langganan" });
}
const radio = (name: string) => screen.getByRole("radio", { name });

describe("Settings", () => {
  it("shows the groups in order, without a Simpan button", async () => {
    await ready();
    const heads = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(heads).toEqual(["Umum", "Tampilan", "Agent", "Langganan", "Notifikasi", "Privasi", "Data"]);
    expect(screen.queryByRole("button", { name: "Simpan" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Kurs USD Ke IDR")).toHaveValue(16300);
    expect(screen.getByRole("switch", { name: "Buka Saat Login" })).not.toBeChecked();
    expect(radio("Sistem")).toBeChecked();
    expect(radio("Sisa")).toBeChecked();
    expect(radio("Jam")).toBeChecked();
    expect(saved()).toHaveLength(0);
  });

  it("settings from before the display preferences existed still load with the defaults", async () => {
    const { theme, quotaDisplay, resetDisplay, ...old } = settingsFixture as Record<string, unknown>;
    backend = { ...old, onboarded: true };
    await ready();
    expect([radio("Sistem"), radio("Sisa"), radio("Jam")].map((r) => r.getAttribute("aria-checked"))).toEqual(["true", "true", "true"]);
  });

  it("every control saves at once, one field per save", async () => {
    await ready();
    const user = userEvent.setup();
    await user.click(radio("Terpakai"));
    await user.click(radio("Hitung Mundur"));
    await user.click(radio("Gelap"));
    await waitFor(() => expect(saved()).toHaveLength(3));
    expect(saved()).toEqual([{ quotaDisplay: "used" }, { resetDisplay: "countdown" }, { theme: "dark" }]);
    expect(radio("Terpakai")).toBeChecked();
    expect(radio("Hitung Mundur")).toBeChecked();
    expect(radio("Gelap")).toBeChecked();
  });

  it("the theme applies to the window without a restart", async () => {
    await ready();
    const user = userEvent.setup();
    await user.click(radio("Gelap"));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    await user.click(radio("Terang"));
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("Kurs saves on blur and on Enter, not on every keystroke", async () => {
    await ready();
    const user = userEvent.setup();
    const rate = screen.getByLabelText("Kurs USD Ke IDR");
    await user.clear(rate);
    await user.type(rate, "17000");
    expect(saved()).toHaveLength(0);
    await user.tab();
    await waitFor(() => expect(saved()).toEqual([{ usdToIdr: 17000 }]));
    await user.clear(rate);
    await user.type(rate, "17500{Enter}");
    await waitFor(() => expect(saved()).toEqual([{ usdToIdr: 17000 }, { usdToIdr: 17500 }]));
  });

  it("an invalid Kurs shows the error and is not saved", async () => {
    await ready();
    const user = userEvent.setup();
    const rate = screen.getByLabelText("Kurs USD Ke IDR");
    await user.clear(rate);
    await user.type(rate, "0");
    await user.tab();
    expect(screen.getByText("Kurs harus lebih dari 0.")).toBeInTheDocument();
    await user.clear(rate);
    await user.tab();
    expect(screen.getByText("Kurs harus lebih dari 0.")).toBeInTheDocument();
    expect(saved()).toHaveLength(0);
    await user.type(rate, "16000{Enter}");
    await waitFor(() => expect(saved()).toEqual([{ usdToIdr: 16000 }]));
    expect(screen.queryByText("Kurs harus lebih dari 0.")).not.toBeInTheDocument();
  });

  it("the Kurs error sits in the row's note and is tied to the input", async () => {
    await ready();
    const user = userEvent.setup();
    const rate = screen.getByLabelText("Kurs USD Ke IDR");
    await user.clear(rate);
    await user.type(rate, "0");
    await user.tab();
    const error = screen.getByText("Kurs harus lebih dari 0.");
    expect(error).toHaveAttribute("role", "alert");
    expect(rate).toHaveAttribute("aria-invalid", "true");
    expect(rate).toHaveAttribute("aria-describedby", error.id);
    expect(error.closest("small")).not.toBeNull(); // in the row's note
    expect(error.closest("span[class~='min-w-0']")?.querySelector("p")).toBeNull(); // no <p> inside the row's <span>
    await user.type(rate, "{Backspace}16000{Enter}");
    await waitFor(() => expect(screen.queryByText("Kurs harus lebih dari 0.")).not.toBeInTheDocument());
    expect(rate).not.toHaveAttribute("aria-describedby");
  });

  it("a plan form's error is tied to its input too", async () => {
    await ready();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Ubah Plan" }));
    const form = screen.getByRole("group", { name: "Plan" });
    const price = within(form).getByLabelText("Harga (USD)");
    await user.clear(price);
    await user.type(price, "-5{Enter}");
    const error = within(form).getByText("Harga tidak boleh negatif.");
    expect(price).toHaveAttribute("aria-describedby", error.id);
  });

  it("the form refuses to close over an error and says why", async () => {
    await ready();
    const user = userEvent.setup();
    const toggle = screen.getByRole("button", { name: "Ubah Plan" });
    await user.click(toggle);
    const form = screen.getByRole("group", { name: "Plan" });
    const price = within(form).getByLabelText("Harga (USD)");
    await user.clear(price);
    await user.type(price, "-5{Enter}");
    await user.click(screen.getByRole("button", { name: "Tutup Plan" }));
    expect(screen.getByRole("group", { name: "Plan" })).toBeInTheDocument();
    const why = screen.getByText("Perbaiki isian yang salah dulu.");
    expect(why).toHaveAttribute("role", "alert");
    expect(screen.getByRole("button", { name: "Tutup Plan" })).toHaveAttribute("aria-describedby", why.id);
    await user.clear(price);
    await user.type(price, "5{Enter}");
    await waitFor(() => expect(screen.queryByText("Perbaiki isian yang salah dulu.")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Tutup Plan" }));
    expect(screen.queryByRole("group", { name: "Plan" })).not.toBeInTheDocument();
  });

  it("a failed save stays at the top of the scrolling window", async () => {
    commands.save_settings = () => {
      throw new Error("disk penuh");
    };
    await ready();
    await userEvent.click(radio("Terpakai"));
    expect((await screen.findByRole("alert")).className).toContain("sticky");
    expect(screen.getByRole("alert").className).toContain("top-0");
  });

  it("a failed save puts the control back and says so", async () => {
    commands.save_settings = () => {
      throw new Error("disk penuh");
    };
    await ready();
    await userEvent.click(radio("Terpakai"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Gagal menyimpan: disk penuh");
    expect(radio("Sisa")).toBeChecked();
  });

  it("agent switches save the FULL enabledAgents map", async () => {
    await ready();
    expect(screen.getByRole("switch", { name: "Claude Code" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "OpenCode" })).not.toBeChecked();
    await userEvent.click(screen.getByRole("switch", { name: "Hermes" }));
    await waitFor(() => expect(saved()).toHaveLength(1));
    expect(saved()[0]).toEqual({ enabledAgents: { cc: true, cx: true, oc: false, hm: false, cr: true } });
    expect(screen.getByRole("switch", { name: "Hermes" })).not.toBeChecked();
  });

  it("each agent row shows its status from the data we have", async () => {
    commands.cursor_session_status = () => ({ connected: true, expiresAt: null });
    commands.scan_status = () => ({ ...status, sources: status.sources.map((s) => (s.agent === "cx" ? { ...s, found: true } : s)) });
    await ready();
    const row = (name: string) => screen.getByRole("switch", { name }).closest("li")!;
    expect(row("Claude Code")).toHaveTextContent("Kuota Aktif");
    expect(row("Codex")).toHaveTextContent("Kuota Aktif");
    expect(row("Cursor")).toHaveTextContent("Terhubung");
    expect(row("OpenCode")).toHaveTextContent("Per Pakai");
    expect(row("Hermes")).toHaveTextContent("Per Pakai");
  });

  it("Cursor without a session is Belum Terhubung, and an agent without a folder says so", async () => {
    commands.cursor_session_status = () => ({ connected: false, expiresAt: null });
    await ready();
    const row = (name: string) => screen.getByRole("switch", { name }).closest("li")!;
    expect(row("Cursor")).toHaveTextContent("Belum Terhubung");
    expect(row("Codex")).toHaveTextContent("Tidak Ditemukan");
    expect(row("Claude Code")).not.toHaveTextContent("Tidak Ditemukan");
  });

  it("Buka Saat Login switches autostart and saves launchAtLogin", async () => {
    await ready();
    await userEvent.click(screen.getByRole("switch", { name: "Buka Saat Login" }));
    await waitFor(() => expect(saved()).toEqual([{ launchAtLogin: true }]));
    expect(auto.enable).toHaveBeenCalledTimes(1);
    expect(auto.disable).not.toHaveBeenCalled();
  });

  it("reflects the real autostart state", async () => {
    auto.isEnabled.mockResolvedValue(true);
    await ready();
    await waitFor(() => expect(screen.getByRole("switch", { name: "Buka Saat Login" })).toBeChecked());
  });

  describe("Langganan", () => {
    it("one row per plan with price, day and agents", async () => {
      await ready();
      const row = screen.getByText("Plan").closest("div[data-row]")!;
      expect(row).toHaveTextContent("$20/bln");
      expect(row).toHaveTextContent("Tanggal 5 · Codex");
      expect(screen.queryByLabelText("Harga (USD)")).not.toBeInTheDocument();
    });

    it("Ubah opens the form under the row; edits save at once", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Ubah Plan" }));
      const form = screen.getByRole("group", { name: "Plan" });
      expect(within(form).getByLabelText("Nama")).toHaveValue("Plan");
      expect(within(form).getByLabelText("Harga (USD)")).toHaveValue(20);
      expect(within(form).getByLabelText("Tanggal Perpanjangan")).toHaveValue(5);
      expect(within(form).getByRole("checkbox", { name: "Codex" })).toBeChecked();
      expect(within(form).getByRole("checkbox", { name: "Claude Code" })).not.toBeChecked();

      const price = within(form).getByLabelText("Harga (USD)");
      await user.clear(price);
      await user.type(price, "25{Enter}");
      await waitFor(() => expect(saved()).toHaveLength(1));
      expect(saved()[0].plans).toEqual([{ id: "p", name: "Plan", priceUsd: 25, renewalDay: 5, agents: ["cx"] }]);
      expect(screen.getByText("$25/bln")).toBeInTheDocument();

      await user.click(within(form).getByRole("checkbox", { name: "Claude Code" }));
      await waitFor(() => expect(saved()).toHaveLength(2));
      expect(saved()[1].plans[0].agents).toEqual(["cx", "cc"]);

      const name = within(form).getByLabelText("Nama");
      await user.clear(name);
      await user.type(name, "Paket Saya");
      await user.tab();
      await waitFor(() => expect(saved()).toHaveLength(3));
      expect(saved()[2].plans[0].name).toBe("Paket Saya");
    });

    it("rejects a bad price, name and day with inline errors and saves nothing", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Ubah Plan" }));
      const form = screen.getByRole("group", { name: "Plan" });
      const price = within(form).getByLabelText("Harga (USD)");
      await user.clear(price);
      await user.type(price, "-5{Enter}");
      expect(within(form).getByText("Harga tidak boleh negatif.")).toBeInTheDocument();
      const day = within(form).getByLabelText("Tanggal Perpanjangan");
      await user.clear(day);
      await user.type(day, "32{Enter}");
      expect(within(form).getByText("Tanggal harus antara 1 dan 31.")).toBeInTheDocument();
      const name = within(form).getByLabelText("Nama");
      await user.clear(name);
      await user.tab();
      expect(within(form).getByText("Nama wajib diisi.")).toBeInTheDocument();
      expect(saved()).toHaveLength(0);
    });

    it("Tambah adds a plan and opens its form; Hapus removes one", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Tambah" }));
      await waitFor(() => expect(saved()).toHaveLength(1));
      const plans = saved()[0].plans;
      expect(plans).toHaveLength(2);
      expect(plans[1]).toMatchObject({ name: "Langganan Baru", priceUsd: 0, renewalDay: 1, agents: [] });
      expect(plans[1].id).toBeTruthy();
      const form = await screen.findByRole("group", { name: "Langganan Baru" });
      // Hapus asks first; Batal keeps the plan and gives focus back to Hapus.
      await user.click(within(form).getByRole("button", { name: "Hapus Langganan Baru" }));
      expect(within(form).getByText("Hapus Langganan Baru?")).toBeInTheDocument();
      expect(within(form).getByRole("button", { name: "Batal" })).toHaveFocus();
      await user.click(within(form).getByRole("button", { name: "Batal" }));
      expect(within(form).getByRole("button", { name: "Hapus Langganan Baru" })).toHaveFocus();
      expect(saved()).toHaveLength(1);
      await user.click(within(form).getByRole("button", { name: "Hapus Langganan Baru" }));
      await user.click(within(form).getByRole("button", { name: "Ya, Hapus" }));
      await waitFor(() => expect(saved()).toHaveLength(2));
      expect(saved()[1].plans.map((p: { id: string }) => p.id)).toEqual(["p"]);
      expect(screen.queryByText("Langganan Baru")).not.toBeInTheDocument();
      // The Hapus that had focus is gone with the form: focus moves to Tambah Langganan, not to <body>.
      expect(screen.getByRole("button", { name: "Tambah" })).toHaveFocus();
    });
  });

  describe("Notifikasi dan Privasi", () => {
    const sw = (name: string) => screen.getByRole("switch", { name });

    it("the three switches start off and save one field each, without asking anything", async () => {
      await ready();
      const names = ["Kuota Di Bawah 15%", "Perkiraan Habis Sebelum Reset", "Sembunyikan Saat Layar Dibagikan"];
      names.forEach((n) => expect(sw(n)).not.toBeChecked());
      const user = userEvent.setup();
      for (const n of names) await user.click(sw(n));
      await waitFor(() => expect(saved()).toEqual([{ notifyLowQuota: true }, { notifyProjectedExhaustion: true }, { hideOnScreenShare: true }]));
      names.forEach((n) => expect(sw(n)).toBeChecked());
      await user.click(sw(names[0]));
      await waitFor(() => expect(saved()[saved().length - 1]).toEqual({ notifyLowQuota: false }));
      expect(invokeMock.mock.calls.some((c) => c[0] === "request_notification_permission")).toBe(false);
    });

    it("explains where to allow notifications and has a test button that only sends one", async () => {
      let sent = 0;
      commands.send_test_notification = () => {
        sent += 1;
        return null;
      };
      await ready();
      expect(screen.getByText("Kalau notifikasi tidak muncul, izinkan Buku Token di System Settings > Notifications.")).toBeInTheDocument();
      expect(screen.getByText("Tes Notifikasi")).toBeInTheDocument();
      expect(screen.getByRole("status", { name: "Tes Notifikasi" })).toBeEmptyDOMElement();
      await userEvent.click(screen.getByRole("button", { name: "Kirim Tes" }));
      await waitFor(() => expect(sent).toBe(1));
      // macOS decides whether to show it, so the app only says it handed it over.
      expect(await screen.findByRole("status", { name: "Tes Notifikasi" })).toHaveTextContent("Terkirim Ke macOS");
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByText("Menu bar berusaha hanya menampilkan logo saat layar dibagikan atau direkam. Deteksi bisa terlambat beberapa detik.")).toBeInTheDocument();
    });

    it("settings from before these switches existed load with them off", async () => {
      const { notifyLowQuota, notifyProjectedExhaustion, hideOnScreenShare, ...old } = settingsFixture as Record<string, unknown>;
      backend = { ...old, onboarded: true };
      await ready();
      expect(sw("Kuota Di Bawah 15%")).not.toBeChecked();
      expect(sw("Sembunyikan Saat Layar Dibagikan")).not.toBeChecked();
    });
  });

  describe("saving in flight", () => {
    type Pending = { patch: any; ok: () => void; fail: (m: string) => void };
    let pending: Pending[];
    beforeEach(() => {
      pending = [];
      commands.save_settings = (a) =>
        new Promise((resolve, reject) => {
          const patch = (a as { settings: object }).settings;
          pending.push({
            patch,
            ok: () => resolve((backend = { ...backend, ...patch })),
            // a stale reply: the server state from before the later edits
            fail: (m) => reject(new Error(m)),
          });
        });
    });
    const settle = (i: number) => act(async () => pending[i].ok());

    it("a reply to an earlier save does not undo a later edit, and the next whole-map patch keeps both", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(screen.getByRole("switch", { name: "Hermes" }));
      await user.click(screen.getByRole("switch", { name: "OpenCode" }));
      await waitFor(() => expect(pending).toHaveLength(1));
      await settle(0);
      expect(screen.getByRole("switch", { name: "Hermes" })).not.toBeChecked();
      expect(screen.getByRole("switch", { name: "OpenCode" })).toBeChecked();
      await waitFor(() => expect(pending).toHaveLength(2));
      await settle(1);
      await user.click(screen.getByRole("switch", { name: "Codex" }));
      await waitFor(() => expect(pending).toHaveLength(3));
      expect(pending[2].patch.enabledAgents).toEqual({ cc: true, cx: false, oc: true, hm: false, cr: true });
    });

    it("a plan edit and a Tambah right after it both reach the saved list", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Ubah Plan" }));
      const price = screen.getByLabelText("Harga (USD)");
      await user.clear(price);
      await user.type(price, "25{Enter}");
      await user.click(screen.getByRole("button", { name: "Tambah" }));
      await waitFor(() => expect(pending).toHaveLength(1));
      await settle(0);
      await waitFor(() => expect(pending).toHaveLength(2));
      expect(pending[1].patch.plans.map((p: { id: string; priceUsd: number }) => [p.id, p.priceUsd]).slice(0, 1)).toEqual([["p", 25]]);
      expect(pending[1].patch.plans).toHaveLength(2);
      await settle(1);
      expect(screen.getByText("$25/bln")).toBeInTheDocument();
    });

    it("a failed save rolls back only a key with no later edit pending", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(radio("Gelap"));
      await user.click(radio("Terang"));
      await user.click(radio("Terpakai"));
      await waitFor(() => expect(pending).toHaveLength(1));
      await act(async () => pending[0].fail("putus"));
      await waitFor(() => expect(pending).toHaveLength(2));
      await settle(1);
      await waitFor(() => expect(pending).toHaveLength(3));
      await act(async () => pending[2].fail("putus lagi"));
      expect(radio("Terang")).toBeChecked();
      expect(radio("Sisa")).toBeChecked();
      expect(await screen.findByRole("alert")).toHaveTextContent("putus lagi");
    });

    it("after two failures on the same key, the screen shows what is really stored", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(radio("Gelap"));
      await user.click(radio("Terang"));
      await waitFor(() => expect(pending).toHaveLength(1));
      await act(async () => pending[0].fail("putus"));
      await waitFor(() => expect(pending).toHaveLength(2));
      await act(async () => pending[1].fail("putus lagi"));
      // "Gelap" was never saved; the stored theme is still Sistem
      await waitFor(() => expect(radio("Sistem")).toBeChecked());
      expect(radio("Gelap")).not.toBeChecked();
      expect(screen.getByRole("alert")).toHaveTextContent("putus lagi");
    });

    it("the reload after a failed save does not land over an edit made while it was pending", async () => {
      await ready();
      const user = userEvent.setup();
      await user.click(radio("Gelap"));
      await waitFor(() => expect(pending).toHaveLength(1));
      let release!: (s: unknown) => void;
      commands.get_settings = () => new Promise((r) => (release = r));
      await act(async () => pending[0].fail("putus"));
      await waitFor(() => expect(release).toBeTypeOf("function"));
      await user.click(radio("Terang")); // edited while the reload is pending (its save queues behind it)
      await act(async () => release({ ...backend, theme: "system" }));
      await waitFor(() => expect(pending).toHaveLength(2));
      expect(radio("Terang")).toBeChecked();
      expect(radio("Sistem")).not.toBeChecked();
    });

    it("focus and settings-opened do not load over a save in flight", async () => {
      await mount();
      await userEvent.click(radio("Terpakai"));
      await waitFor(() => expect(pending).toHaveLength(1));
      const before = invokeMock.mock.calls.length;
      win.focus!({ payload: true });
      emit("settings-opened");
      await waitFor(() => expect(invokeMock.mock.calls.length).toBeGreaterThan(before));
      await new Promise((r) => setTimeout(r, 20));
      expect(radio("Terpakai")).toBeChecked();
    });
  });

  describe("Buka Saat Login", () => {
    it("enable() failing leaves the switch where it was and shows the error", async () => {
      auto.enable.mockRejectedValue(new Error("tidak diizinkan"));
      await ready();
      await userEvent.click(screen.getByRole("switch", { name: "Buka Saat Login" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("tidak diizinkan");
      expect(screen.getByRole("switch", { name: "Buka Saat Login" })).not.toBeChecked();
      expect(saved()).toHaveLength(0);
    });

    it("a failed save after the OS changed puts the OS state back", async () => {
      commands.save_settings = () => {
        throw new Error("disk penuh");
      };
      await ready();
      await userEvent.click(screen.getByRole("switch", { name: "Buka Saat Login" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("disk penuh");
      expect(auto.enable).toHaveBeenCalledTimes(1);
      await waitFor(() => expect(auto.disable).toHaveBeenCalledTimes(1));
      expect(screen.getByRole("switch", { name: "Buka Saat Login" })).not.toBeChecked();
    });

    it("ignores clicks while one is in flight", async () => {
      let done!: () => void;
      auto.enable.mockImplementation(() => new Promise<void>((r) => (done = r)));
      await ready();
      const sw = screen.getByRole("switch", { name: "Buka Saat Login" });
      await userEvent.click(sw);
      await userEvent.click(sw);
      await act(async () => done());
      await waitFor(() => expect(saved()).toHaveLength(1));
      expect(auto.enable).toHaveBeenCalledTimes(1);
      expect(auto.disable).not.toHaveBeenCalled();
    });
  });

  it("Tutup with an invalid half-typed field keeps the form open with the error", async () => {
    await ready();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Ubah Plan" }));
    const price = screen.getByLabelText("Harga (USD)");
    await user.clear(price);
    await user.type(price, "-5");
    await user.click(screen.getByRole("button", { name: "Tutup Plan" }));
    expect(screen.getByRole("group", { name: "Plan" })).toBeInTheDocument();
    expect(screen.getByText("Harga tidak boleh negatif.")).toBeInTheDocument();
    await user.clear(price);
    await user.type(price, "10");
    await user.click(screen.getByRole("button", { name: "Tutup Plan" }));
    expect(screen.queryByRole("group", { name: "Plan" })).not.toBeInTheDocument();
    expect(saved()[saved().length - 1].plans[0].priceUsd).toBe(10);
  });

  describe("Data", () => {
    it("pricing version, folder count with a list behind Lihat, skipped lines", async () => {
      await ready();
      expect(screen.getByText("2026-10-30")).toBeInTheDocument();
      expect(screen.getByText("9 Folder")).toBeInTheDocument();
      expect(screen.queryByText("/Users/dev/.claude/projects")).not.toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "Lihat" }));
      expect(screen.getByText("/Users/dev/.claude/projects")).toBeInTheDocument();
      expect(screen.getByText("/Users/dev/.cursor/projects")).toHaveClass("line-through");
      expect(screen.getByText("/Users/dev/.gemini/antigravity/conversations")).not.toHaveClass("line-through");
      expect(screen.getByText("/Users/dev/.gemini/antigravity-ide/conversations")).toHaveClass("line-through");
      expect(screen.getByText("/Users/dev/.gemini/antigravity-cli/conversation_summaries.db")).not.toHaveClass("line-through");
      expect(screen.getAllByText("Antigravity").length).toBeGreaterThanOrEqual(4); // row, and one per folder
      const skipped = screen.getByText("Baris Dilewati").closest("div[data-row]")!;
      expect(skipped).toHaveTextContent("3");
    });

    it("hides the skipped row when nothing was skipped", async () => {
      commands.scan_status = () => ({ ...status, skipped: 0 });
      await ready();
      expect(screen.queryByText("Baris Dilewati")).not.toBeInTheDocument();
    });

    it("Perbarui shows loading, then the new version", async () => {
      await ready();
      let done!: (v: string) => void;
      commands.update_pricing = () => new Promise<string>((r) => (done = r));
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Perbarui" }));
      expect(screen.getByRole("button", { name: /Memperbarui/ })).toBeDisabled();
      done("2026-11-02");
      expect(await screen.findByText("Daftar harga diperbarui ke 2026-11-02.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Perbarui" })).toBeEnabled();
    });

    it("Perbarui shows the error", async () => {
      await ready();
      commands.update_pricing = () => {
        throw new Error("offline");
      };
      await userEvent.click(screen.getByRole("button", { name: "Perbarui" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("Gagal memperbarui daftar harga: offline");
    });
  });

  const changeBackend = () => {
    backend = { ...backend, usdToIdr: 18000, theme: "dark" };
    commands.scan_status = () => ({
      ...status,
      skipped: 7,
      pricingVersion: "2026-12-01",
      sources: status.sources.map((s) => ({ ...s, found: s.agent === "cx" })),
    });
  };
  const mount = async () => {
    const r = render(<Settings />);
    await screen.findByRole("heading", { name: "Langganan" });
    await waitFor(() => expect(win.focus).not.toBeNull());
    await waitFor(() => expect(listenMock).toHaveBeenCalledWith("settings-opened", expect.any(Function)));
    return r;
  };

  it("settings-opened reloads everything and closes the open forms", async () => {
    const { unmount } = await mount();
    await userEvent.click(screen.getByRole("button", { name: "Ubah Plan" }));
    changeBackend();
    auto.isEnabled.mockResolvedValue(true);
    emit("settings-opened");
    await waitFor(() => expect(screen.getByLabelText("Kurs USD Ke IDR")).toHaveValue(18000));
    expect(screen.getByText("2026-12-01")).toBeInTheDocument();
    expect(radio("Gelap")).toBeChecked();
    expect(screen.getByRole("switch", { name: "Buka Saat Login" })).toBeChecked();
    expect(screen.queryByRole("group", { name: "Plan" })).not.toBeInTheDocument();
    unmount();
    await waitFor(() => expect(win.unlisten).toHaveBeenCalled());
  });

  it("focus gain picks up new settings", async () => {
    await mount();
    changeBackend();
    win.focus!({ payload: true });
    await waitFor(() => expect(screen.getByLabelText("Kurs USD Ke IDR")).toHaveValue(18000));
  });

  it("a failed reload shows an alert over the rendered form, and Coba Lagi recovers", async () => {
    await mount();
    commands.scan_status = () => {
      throw new Error("putus");
    };
    emit("settings-opened");
    expect(await screen.findByRole("alert")).toHaveTextContent("Gagal memuat pengaturan: Error: putus");
    expect(screen.getByRole("heading", { name: "Langganan" })).toBeInTheDocument();
    commands.scan_status = () => status;
    await userEvent.click(screen.getByRole("button", { name: "Coba Lagi" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });
});

describe("Akun Cursor", () => {
  const COOKIE = "user_01%3A%3Asuper-secret-cookie";
  type Session = {
    connected: boolean;
    expiresAt: string | null;
    keychainDenied?: boolean;
    lastReason?: string | null;
    historyFailed?: boolean;
  };
  let session: Session;
  const calls = (name: string) => invokeMock.mock.calls.filter((c) => c[0] === name);

  beforeEach(() => {
    session = { connected: false, expiresAt: null };
    commands.cursor_session_status = () => session;
    commands.connect_cursor = () => (session = { connected: true, expiresAt: "2027-01-15T00:00:00Z" });
    commands.disconnect_cursor = () => {
      session = { connected: false, expiresAt: null };
      return null;
    };
  });

  const open = async () => userEvent.click(await screen.findByRole("button", { name: "Kelola Cursor" }));

  it("not connected: steps, a masked input and a Hubungkan button that waits for input", async () => {
    await ready();
    await open();
    expect(screen.getByRole("heading", { name: "Akun Cursor" })).toBeInTheDocument();
    expect(screen.getByText(/cursor\.com\/dashboard/)).toBeInTheDocument();
    const input = screen.getByLabelText("Cookie Sesi");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveAttribute("autocomplete", "off");
    expect(screen.getByRole("button", { name: "Hubungkan" })).toBeDisabled();
    await userEvent.type(input, "x");
    expect(screen.getByRole("button", { name: "Hubungkan" })).toBeEnabled();
  });

  it("connecting sends the cookie once, clears the input and never shows it", async () => {
    await ready();
    await open();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Cookie Sesi"), COOKIE);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    const line = await screen.findByText("Akun Terhubung");
    expect(calls("connect_cursor")).toEqual([["connect_cursor", { cookie: COOKIE }]]);
    expect(screen.getByText(/Berlaku Sampai.*2027/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Cookie Sesi")).not.toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("super-secret-cookie");
    // The Hubungkan that had focus is gone: the always-present status line, which announces it, takes focus.
    expect(line).toHaveAttribute("role", "status");
    await waitFor(() => expect(line).toHaveFocus());
    expect(screen.getByRole("button", { name: "Putuskan" })).toBeInTheDocument();
    // Putuskan asks first. The old cookie must not refill the form once the user disconnects.
    await user.click(screen.getByRole("button", { name: "Putuskan" }));
    expect(calls("disconnect_cursor")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Ya, Putuskan" }));
    expect(await screen.findByLabelText("Cookie Sesi")).toHaveValue("");
    const parted = screen.getByRole("status", { name: "" });
    expect(parted).toHaveTextContent("Akun Terputus");
    await waitFor(() => expect(parted).toHaveFocus());
  });

  it("Enter in the cookie field connects, like the button", async () => {
    await ready();
    await open();
    await userEvent.type(screen.getByLabelText("Cookie Sesi"), COOKIE + "{Enter}");
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
    expect(calls("connect_cursor")).toHaveLength(1);
  });

  it("Batal on the Putuskan question keeps the account and gives focus back", async () => {
    session = { connected: true, expiresAt: null };
    await ready();
    await open();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Putuskan" }));
    expect(screen.getByRole("group", { name: /Putuskan akun ini/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Batal" }));
    expect(screen.getByRole("button", { name: "Putuskan" })).toHaveFocus();
    expect(calls("disconnect_cursor")).toHaveLength(0);
    expect(screen.getByText("Akun Terhubung")).toBeInTheDocument();
  });

  it("Tempel Ulang reveals the form while connected; success replaces, a rejection keeps the connection", async () => {
    session = { connected: true, expiresAt: "2027-01-15T00:00:00Z" };
    commands.connect_cursor = () => {
      throw "Cookie Tidak Valid";
    };
    await ready();
    await open();
    const user = userEvent.setup();
    expect(screen.queryByLabelText("Cookie Sesi")).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Tempel Ulang" }));
    await user.type(screen.getByLabelText("Cookie Sesi"), COOKIE);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Cookie Tidak Valid");
    expect(screen.getByText("Akun Terhubung")).toBeInTheDocument();
    expect(calls("disconnect_cursor")).toHaveLength(0);

    commands.connect_cursor = () => (session = { connected: true, expiresAt: "2028-02-01T00:00:00Z" });
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    await waitFor(() => expect(screen.getByText(/Berlaku Sampai.*2028/)).toBeInTheDocument());
    expect(screen.queryByLabelText("Cookie Sesi")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows Sudah Berakhir once the expiry date has passed", async () => {
    session = { connected: true, expiresAt: "2020-01-01T00:00:00Z" };
    await ready();
    await open();
    expect(await screen.findByText("Sudah Berakhir")).toBeInTheDocument();
    expect(screen.queryByText(/Berlaku Sampai/)).not.toBeInTheDocument();
  });

  it("is disabled while connecting", async () => {
    let done!: () => void;
    commands.connect_cursor = () => new Promise((r) => (done = () => r({ connected: true, expiresAt: null })));
    await ready();
    await open();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Cookie Sesi"), "abc");
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    // aria-disabled, not disabled: the pressed button keeps keyboard focus, and the status line says what runs.
    const busy = screen.getByRole("button", { name: "Menghubungkan…" });
    expect(busy).toHaveAttribute("aria-disabled", "true");
    expect(busy).toHaveFocus();
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent("Menghubungkan…");
    await user.keyboard("{Enter}");
    expect(calls("connect_cursor")).toHaveLength(1);
    done();
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
  });

  it("a rejected cookie shows the error in an alert, keeps the form and does not echo the cookie", async () => {
    commands.connect_cursor = () => {
      throw "Cookie Tidak Valid";
    };
    await ready();
    await open();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Cookie Sesi"), COOKIE);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    const alert = await screen.findByText("Cookie Tidak Valid");
    expect(alert).toHaveAttribute("role", "alert");
    expect(screen.queryByText("Akun Terhubung")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Cookie Sesi")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("super-secret-cookie");
  });

  it("connected on load shows Terhubung without a date when none is known, and Putuskan returns to the form", async () => {
    session = { connected: true, expiresAt: null };
    await ready();
    await open();
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
    expect(screen.queryByText(/Berlaku Sampai/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Putuskan" }));
    await userEvent.click(screen.getByRole("button", { name: "Ya, Putuskan" }));
    expect(await screen.findByRole("button", { name: "Hubungkan" })).toBeInTheDocument();
    expect(calls("disconnect_cursor")).toHaveLength(1);
  });

  it("Putuskan clears text left in the form so it does not reappear", async () => {
    session = { connected: true, expiresAt: null };
    await ready();
    await open();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Tempel Ulang" }));
    await user.type(screen.getByLabelText("Cookie Sesi"), "sisa-ketikan");
    await user.click(screen.getByRole("button", { name: "Putuskan" }));
    await user.click(screen.getByRole("button", { name: "Ya, Putuskan" }));
    expect(await screen.findByLabelText("Cookie Sesi")).toHaveValue("");
  });

  it("settings still load when the session status cannot be read", async () => {
    delete commands.cursor_session_status;
    await ready();
    await open();
    expect(screen.getByRole("button", { name: "Hubungkan" })).toBeInTheDocument();
  });

  it("a missing Cursor folder is listed struck through but is not 'not found' on the agent row", async () => {
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Lihat" }));
    expect(screen.getByText("/Users/dev/.cursor/projects")).toHaveClass("line-through");
    // Struck through is not enough for a screen reader: every missing folder also says so, found ones do not.
    const folder = (path: string) => screen.getByText(path).closest("li")!;
    expect(folder("/Users/dev/.cursor/projects").querySelector(".sr-only")).toHaveTextContent("Tidak Ditemukan");
    expect(folder("/Users/dev/.codex/sessions").querySelector(".sr-only")).toHaveTextContent("Tidak Ditemukan");
    expect(folder("/Users/dev/.claude/projects").querySelector(".sr-only")).toBeNull();
    // On the agent rows only Codex is "not found": Cursor's usage comes from cursor.com.
    expect(screen.getByRole("switch", { name: "Codex" }).closest("li")).toHaveTextContent("Tidak Ditemukan");
    expect(screen.getByRole("switch", { name: "Cursor" }).closest("li")).not.toHaveTextContent("Tidak Ditemukan");
  });

  it("the connected view shows the current reason and a failed history sync", async () => {
    session = { connected: true, expiresAt: null, lastReason: "Sedang Offline", historyFailed: true };
    await ready();
    await open();
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
    expect(screen.getByText("Sedang Offline")).toBeInTheDocument();
    expect(screen.getByText("Riwayat Cursor Gagal Disinkronkan")).toBeInTheDocument();
  });

  it("a refused Keychain shows Akses Keychain Ditolak instead of the empty form", async () => {
    session = { connected: false, expiresAt: null, keychainDenied: true };
    await ready();
    await open();
    expect(await screen.findByText("Akses Keychain Ditolak")).toBeInTheDocument();
    expect(screen.queryByLabelText("Cookie Sesi")).not.toBeInTheDocument();
    expect(screen.queryByText("Akun Terhubung")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Tempel Ulang" }));
    expect(screen.getByLabelText("Cookie Sesi")).toBeInTheDocument();
  });

  it("reads the Cursor status on open and after its own actions, not on every focus", async () => {
    session = { connected: true, expiresAt: null };
    render(<Settings />);
    await open();
    await screen.findByText("Akun Terhubung");
    await waitFor(() => expect(win.focus).not.toBeNull());
    expect(calls("cursor_session_status")).toHaveLength(1);
    win.focus!({ payload: true });
    win.focus!({ payload: true });
    await waitFor(() => expect(calls("get_settings").length).toBeGreaterThan(1));
    expect(calls("cursor_session_status")).toHaveLength(1);
    emit("settings-opened");
    await waitFor(() => expect(calls("cursor_session_status")).toHaveLength(2));
  });

  it("the plan added by the first connect shows up, and later saves keep it", async () => {
    await ready();
    await open();
    const user = userEvent.setup();
    const plan = { id: "cursor-pro", name: "Cursor Pro", priceUsd: 20, renewalDay: 5, agents: ["cr"] };
    backend = { ...backend, plans: [...settingsFixture.plans, plan] };
    await user.type(screen.getByLabelText("Cookie Sesi"), COOKIE);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    expect(await screen.findByText("Cursor Pro")).toBeInTheDocument();
    expect(screen.getAllByText("Cursor Pro")).toHaveLength(1);
    await user.click(screen.getByRole("radio", { name: "Terpakai" }));
    const rate = screen.getByLabelText("Kurs USD Ke IDR");
    await user.clear(rate);
    await user.type(rate, "17000{Enter}");
    await waitFor(() => expect(saved()).toHaveLength(2));
    // Nothing the screen saves carries a stale plan list: Cursor Pro is never dropped.
    expect(saved().every((s: { plans?: { id: string }[] }) => !s.plans || s.plans.some((p) => p.id === "cursor-pro"))).toBe(true);
    await user.click(screen.getByRole("button", { name: "Tambah" }));
    await waitFor(() => expect(saved()).toHaveLength(3));
    expect(saved()[2].plans.map((p: { id: string }) => p.id)).toEqual(["p", "cursor-pro", expect.any(String)]);
  });
});

describe("Settings language change", () => {
  it("rewords a pricing result worded in the old language, and re-reads the reasons without the Keychain", async () => {
    commands.update_pricing = () => {
      throw new Error("offline");
    };
    const calls = (name: string) => invokeMock.mock.calls.filter((c) => c[0] === name);
    let reason = "Sedang Offline";
    commands.cursor_session_status = () => ({ connected: true, expiresAt: null, keychainDenied: false, lastReason: reason, historyFailed: false });
    commands.cursor_last_reason = () => reason;
    commands.openrouter_session_status = () => ({ connected: true, keychainDenied: false, lastReason: reason });
    await ready();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Perbarui" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Gagal memperbarui daftar harga: offline");
    await user.click(screen.getByRole("button", { name: "Kelola Cursor" }));
    expect(await screen.findByText("Sedang Offline")).toBeInTheDocument();
    expect(calls("cursor_session_status")).toHaveLength(1);
    expect(calls("openrouter_session_status")).toHaveLength(1);

    reason = "Offline";
    await user.click(radio("English"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not update the price list: offline"));
    await waitFor(() => expect(screen.getByText("Offline")).toBeInTheDocument());
    expect(calls("cursor_session_status")).toHaveLength(1);
    expect(calls("cursor_last_reason").length).toBeGreaterThan(0);
    expect(calls("openrouter_session_status")).toHaveLength(2);
  });

  it("a Cursor connect in flight survives settings-opened, like OpenRouter's", async () => {
    let done!: () => void;
    commands.cursor_session_status = () => ({ connected: false, expiresAt: null, keychainDenied: false, lastReason: null, historyFailed: false });
    commands.connect_cursor = () => new Promise((r) => (done = () => r({ connected: true, expiresAt: null })));
    await ready();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Kelola Cursor" }));
    await user.type(screen.getByLabelText("Cookie Sesi"), "abc");
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    act(() => emit("settings-opened"));
    await user.click(screen.getByRole("button", { name: "Kelola Cursor" }));
    expect(screen.getByRole("button", { name: "Menghubungkan…" })).toBeDisabled();
    await act(async () => done());
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
  });
});

describe("Settings OpenRouter and Antigravity", () => {
  const KEY = "sk-or-v1-test";
  let session: { connected: boolean; keychainDenied: boolean; lastReason: string | null };
  const calls = (name: string) => invokeMock.mock.calls.filter((c) => c[0] === name);
  const agentRow = (name: string) => screen.getByRole("switch", { name }).closest("div[data-row]") as HTMLElement;
  const open = async () => userEvent.click(await screen.findByRole("button", { name: "Kelola OpenRouter" }));

  beforeEach(() => {
    session = { connected: false, keychainDenied: false, lastReason: null };
    commands.openrouter_session_status = () => session;
    commands.connect_openrouter = () => (session = { connected: true, keychainDenied: false, lastReason: null });
    commands.disconnect_openrouter = () => {
      session = { connected: false, keychainDenied: false, lastReason: null };
      return null;
    };
  });

  it("both rows sit in the Agent group after the five agents, with their status", async () => {
    await ready();
    const group = screen.getByRole("heading", { name: "Agent" }).nextElementSibling as HTMLElement;
    const names = within(group).getAllByRole("switch").map((s) => s.getAttribute("aria-label"));
    expect(names).toEqual(["Claude Code", "Codex", "OpenCode", "Hermes", "Cursor", "OpenRouter", "Antigravity"]);
    expect(agentRow("OpenRouter")).toHaveTextContent("Belum Terhubung");
    expect(agentRow("Antigravity")).toHaveTextContent("Aktivitas Saja");
    expect(agentRow("Antigravity").querySelector("svg use")).toHaveAttribute("href", "#lg-antigravity");
    expect(within(agentRow("Antigravity")).queryByRole("button")).not.toBeInTheDocument();
  });

  it("connected shows Terhubung", async () => {
    session = { connected: true, keychainDenied: false, lastReason: null };
    await ready();
    await waitFor(() => expect(agentRow("OpenRouter")).toHaveTextContent("Terhubung"));
    expect(agentRow("OpenRouter")).not.toHaveTextContent("Belum");
  });

  it("each switch saves its own field and nothing else", async () => {
    await ready();
    const user = userEvent.setup();
    expect(screen.getByRole("switch", { name: "OpenRouter" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Antigravity" })).toBeChecked();
    await user.click(screen.getByRole("switch", { name: "OpenRouter" }));
    await user.click(screen.getByRole("switch", { name: "Antigravity" }));
    await waitFor(() => expect(saved()).toHaveLength(2));
    expect(saved()).toEqual([{ openrouterEnabled: false }, { antigravityEnabled: false }]);
  });

  it("settings from before these switches existed load with both on", async () => {
    const { openrouterEnabled, antigravityEnabled, ...old } = settingsFixture as Record<string, unknown>;
    backend = { ...old, onboarded: true };
    await ready();
    expect(screen.getByRole("switch", { name: "OpenRouter" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Antigravity" })).toBeChecked();
  });

  it("Kelola opens the form: steps, a masked input, Hubungkan that waits for input", async () => {
    await ready();
    await open();
    expect(screen.getByRole("heading", { name: "Akun OpenRouter" })).toBeInTheDocument();
    expect(screen.getByText(/openrouter\.ai\/settings\/keys/)).toBeInTheDocument();
    const input = screen.getByLabelText("API Key");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveAttribute("autocomplete", "off");
    expect(screen.getByRole("button", { name: "Hubungkan" })).toBeDisabled();
    await userEvent.type(input, "x");
    expect(screen.getByRole("button", { name: "Hubungkan" })).toBeEnabled();
    expect(screen.getByText(/hanya dikirim ke openrouter\.ai/)).toBeInTheDocument();
  });

  it("connecting sends the key once, clears the field and never shows it", async () => {
    await ready();
    await open();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("API Key"), KEY);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
    expect(calls("connect_openrouter")).toEqual([["connect_openrouter", { key: KEY }]]);
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain(KEY);
    expect(agentRow("OpenRouter")).toHaveTextContent("Terhubung");
    // Putuskan must not refill the form with the old key.
    await user.click(screen.getByRole("button", { name: "Putuskan" }));
    await user.click(screen.getByRole("button", { name: "Ya, Putuskan" }));
    expect(calls("disconnect_openrouter")).toHaveLength(1);
    expect(await screen.findByLabelText("API Key")).toHaveValue("");
    expect(agentRow("OpenRouter")).toHaveTextContent("Belum Terhubung");
  });

  it("Tempel Ulang reveals the form while connected; a rejection keeps the connection and does not echo the key", async () => {
    session = { connected: true, keychainDenied: false, lastReason: "Sedang Offline" };
    commands.connect_openrouter = () => {
      throw "Key Tidak Valid";
    };
    await ready();
    await open();
    const user = userEvent.setup();
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
    expect(screen.getByText("Sedang Offline")).toBeInTheDocument();
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tempel Ulang" }));
    await user.type(screen.getByLabelText("API Key"), KEY);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Key Tidak Valid");
    expect(screen.getByText("Akun Terhubung")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(KEY);
    expect(calls("disconnect_openrouter")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Batal" }));
    expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
  });

  it("a refused Keychain says so and asks to paste again or disconnect", async () => {
    session = { connected: false, keychainDenied: true, lastReason: null };
    await ready();
    await open();
    expect(await screen.findByText("Akses Keychain Ditolak")).toBeInTheDocument();
    expect(agentRow("OpenRouter")).toHaveTextContent("Akses Ditolak");
    expect(screen.getByRole("button", { name: "Putuskan" })).toBeInTheDocument();
  });

  it("is disabled while connecting", async () => {
    let done!: () => void;
    commands.connect_openrouter = () => new Promise((r) => (done = () => r({ connected: true, keychainDenied: false, lastReason: null })));
    await ready();
    await open();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("API Key"), KEY);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    const busy = screen.getByRole("button", { name: "Menghubungkan…" });
    expect(busy).toHaveAttribute("aria-disabled", "true");
    expect(busy).toHaveFocus();
    done();
    expect(await screen.findByText("Akun Terhubung")).toBeInTheDocument();
  });

  it("closing the form while connecting does not re-enable Putuskan, and the answer is kept", async () => {
    session = { connected: true, keychainDenied: false, lastReason: null };
    let reject!: (e: string) => void;
    commands.connect_openrouter = () => new Promise((_r, j) => (reject = j));
    await ready();
    await open();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Tempel Ulang" }));
    await user.type(screen.getByLabelText("API Key"), KEY);
    await user.click(screen.getByRole("button", { name: "Hubungkan" }));
    expect(screen.getByRole("button", { name: "Putuskan" })).toHaveAttribute("aria-disabled", "true");
    await user.click(screen.getByRole("button", { name: "Kelola OpenRouter" })); // close
    await user.click(screen.getByRole("button", { name: "Kelola OpenRouter" })); // reopen
    expect(screen.getByRole("button", { name: "Putuskan" })).toHaveAttribute("aria-disabled", "true");
    expect(calls("disconnect_openrouter")).toHaveLength(0);
    await act(async () => reject("Key Tidak Valid"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Key Tidak Valid");
    await user.click(screen.getByRole("button", { name: "Kelola OpenRouter" }));
    await user.click(screen.getByRole("button", { name: "Kelola OpenRouter" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Key Tidak Valid"); // not lost by remounting
    expect(screen.getByRole("button", { name: "Putuskan" })).not.toHaveAttribute("aria-disabled");
  });

  it("tells to paste the key the tools already use", async () => {
    await ready();
    await open();
    expect(screen.getByText(/key yang sudah dipakai tool-mu/)).toBeInTheDocument();
    expect(screen.getByText(/key baru menunjukkan \$0/)).toBeInTheDocument();
  });

  it("reopening the window drops a half-typed key", async () => {
    await ready();
    await open();
    await userEvent.type(screen.getByLabelText("API Key"), KEY);
    act(() => emit("settings-opened"));
    await waitFor(() => expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument());
    await open();
    expect(screen.getByLabelText("API Key")).toHaveValue("");
  });
});
