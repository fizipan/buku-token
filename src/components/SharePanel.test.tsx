import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import settingsFixture from "../test/fixtures/settings.json";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { ShareRequest } from "../lib/api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { TabActiveContext } from "../hooks/useTabData";
import { SharePanel } from "./SharePanel";

const PNG = "data:image/png;base64,AAAA";
const calls = (cmd: string) => invokeMock.mock.calls.filter((c) => c[0] === cmd).map((c) => (c[1] as { request: ShareRequest }).request);
const lastPreview = () => {
  const all = calls("share_preview");
  return all[all.length - 1];
};

/** The app's own theme is the `dark` class on <html>, not the macOS setting. */
const setDark = (dark: boolean) => document.documentElement.classList.toggle("dark", dark);

async function open(onClose = vi.fn()) {
  const view = render(<SharePanel onClose={onClose} />);
  await screen.findByRole("img", { name: /Pratinjau/ });
  return { ...view, onClose };
}

beforeEach(() => {
  resetTauri();
  setDark(false);
  commands.get_settings = () => settingsFixture; // cc on, oc off, the rest count as on
  commands.share_preview = () => PNG;
  commands.share_copy = () => null;
  commands.share_save = () => "buku-token-2026-10-31.png";
});
afterEach(() => {
  vi.unstubAllGlobals();
  setDark(false);
});

describe("SharePanel preview", () => {
  it("asks for the last 30 days of every agent with the cost shown, and previews it", async () => {
    await open();
    expect(calls("share_preview")).toEqual([{ period: "30d", agent: null, hideCost: false, dark: false }]);
    const img = screen.getByRole("img", { name: "Pratinjau pemakaian 30 Hari Terakhir" });
    expect(img).toHaveAttribute("src", PNG);
    expect(screen.getByRole("heading", { name: "Bagikan Pemakaian" })).toBeInTheDocument();
  });

  it("the card follows the app's theme, even when macOS says otherwise", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: true, media: query, addEventListener() {}, removeEventListener() {} }));
    await open();
    expect(lastPreview().dark).toBe(false);
    vi.unstubAllGlobals();
    setDark(true);
    render(<SharePanel onClose={() => {}} />);
    await waitFor(() => expect(lastPreview().dark).toBe(true));
  });

  it("draws again when the theme changes while it is open", async () => {
    await open();
    expect(lastPreview().dark).toBe(false);
    act(() => setDark(true));
    await waitFor(() => expect(lastPreview().dark).toBe(true));
  });

  it("draws again on data-updated, and reads the switched-on agents again", async () => {
    await open();
    const before = calls("share_preview").length;
    act(() => emit("data-updated"));
    await waitFor(() => expect(calls("share_preview").length).toBe(before + 1));
  });

  it("goes back to All Agents when the chosen agent gets switched off", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("combobox", { name: "Isi" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await waitFor(() => expect(lastPreview().agent).toBe("cx"));
    commands.get_settings = () => ({ ...settingsFixture, enabledAgents: { ...settingsFixture.enabledAgents, cx: false } });
    act(() => emit("data-updated"));
    await waitFor(() => expect(lastPreview().agent).toBe(null));
    expect(screen.getByRole("combobox", { name: "Isi" })).toHaveTextContent("Semua Agent");
  });

  it("the period, content and hide-cost controls change the request", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("button", { name: "Hari Ini" }));
    await waitFor(() => expect(lastPreview().period).toBe("today"));
    expect(screen.getByRole("button", { name: "Hari Ini" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "30 Hari" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "7 Hari" }));
    await waitFor(() => expect(lastPreview().period).toBe("7d"));
    expect(await screen.findByRole("img", { name: "Pratinjau pemakaian 7 Hari Terakhir" })).toBeInTheDocument();

    await user.click(screen.getByRole("combobox", { name: "Isi" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await waitFor(() => expect(lastPreview().agent).toBe("cx"));

    await user.click(screen.getByRole("switch", { name: "Sembunyikan Biaya" }));
    await waitFor(() => expect(lastPreview().hideCost).toBe(true));
    expect(lastPreview()).toEqual({ period: "7d", agent: "cx", hideCost: true, dark: false });
  });

  it("the content select lists All Agents and only the agents that are switched on", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("combobox", { name: "Isi" }));
    const names = (await screen.findAllByRole("option")).map((o) => o.textContent);
    expect(names).toEqual(["Semua Agent", "Claude Code", "Codex", "Hermes", "Cursor"]);
  });

  it("keeps the old preview while a new one loads, and ignores a stale response", async () => {
    const user = userEvent.setup();
    const pending: ((v: string) => void)[] = [];
    await open();
    commands.share_preview = () => new Promise<string>((r) => pending.push(r));
    await user.click(screen.getByRole("button", { name: "Hari Ini" }));
    await user.click(screen.getByRole("button", { name: "7 Hari" }));
    await waitFor(() => expect(pending).toHaveLength(2));
    expect(screen.getByRole("img", { name: /Pratinjau/ })).toHaveAttribute("src", PNG);
    pending[1]("data:image/png;base64,SEVEN");
    await waitFor(() => expect(screen.getByRole("img", { name: /Pratinjau/ })).toHaveAttribute("src", "data:image/png;base64,SEVEN"));
    pending[0]("data:image/png;base64,TODAY");
    await act(async () => {
      await new Promise((r) => setTimeout(r));
    });
    expect(screen.getByRole("img", { name: /Pratinjau/ })).toHaveAttribute("src", "data:image/png;base64,SEVEN");
  });

  it("a failed preview is announced as an alert", async () => {
    commands.share_preview = () => Promise.reject("Gagal membuat gambar");
    render(<SharePanel onClose={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Gagal membuat gambar");
  });
});

describe("SharePanel copy and save", () => {
  it("copy sends the current request and says Tersalin in a polite live region", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("switch", { name: "Sembunyikan Biaya" }));
    await user.click(screen.getByRole("button", { name: "Salin Gambar" }));
    await waitFor(() => expect(calls("share_copy")).toEqual([{ period: "30d", agent: null, hideCost: true, dark: false }]));
    const status = await screen.findByText("Tersalin");
    expect(status.closest("[aria-live]")).toHaveAttribute("aria-live", "polite");
  });

  it("save says where the file went", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("button", { name: "Simpan PNG" }));
    expect(await screen.findByText("Tersimpan Di Downloads: buku-token-2026-10-31.png")).toBeInTheDocument();
    expect(calls("share_save")).toHaveLength(1);
  });

  it("a failed copy or save shows the error as an alert and no success text", async () => {
    const user = userEvent.setup();
    commands.share_copy = () => Promise.reject("Gagal menyalin gambar");
    commands.share_save = () => Promise.reject("Gagal menyimpan gambar ke Downloads");
    await open();
    await user.click(screen.getByRole("button", { name: "Salin Gambar" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Gagal menyalin gambar");
    await user.click(screen.getByRole("button", { name: "Simpan PNG" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Gagal menyimpan gambar ke Downloads"));
    expect(screen.queryByText("Tersalin")).not.toBeInTheDocument();
  });

  it("a redraw on data-updated keeps the status that was already said", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("button", { name: "Salin Gambar" }));
    await screen.findByText("Tersalin");
    const before = calls("share_preview").length;
    act(() => emit("data-updated"));
    await waitFor(() => expect(calls("share_preview").length).toBe(before + 1));
    expect(screen.getByText("Tersalin")).toBeInTheDocument();
  });

  it("a new choice clears the old status", async () => {
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("button", { name: "Salin Gambar" }));
    await screen.findByText("Tersalin");
    await user.click(screen.getByRole("button", { name: "Hari Ini" }));
    await waitFor(() => expect(screen.queryByText("Tersalin")).not.toBeInTheDocument());
  });
});

describe("SharePanel while busy", () => {
  it("locks the period, content and hide-cost controls during copy and save", async () => {
    const user = userEvent.setup();
    let done: () => void = () => {};
    await open();
    commands.share_copy = () => new Promise<null>((r) => (done = () => r(null)));
    await user.click(screen.getByRole("button", { name: "Salin Gambar" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Hari Ini" })).toHaveAttribute("aria-disabled", "true"));
    expect(screen.getByRole("button", { name: "7 Hari" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("combobox", { name: "Isi" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Sembunyikan Biaya" })).toBeDisabled();
    // The pressed button is not disabled for real, so keyboard focus stays on it, and a second press does nothing.
    const copy = screen.getByRole("button", { name: "Salin Gambar" });
    expect(copy).toHaveAttribute("aria-disabled", "true");
    expect(copy).toHaveFocus();
    await user.click(copy);
    await user.click(screen.getByRole("button", { name: "Hari Ini" }));
    expect(calls("share_copy")).toHaveLength(1);
    expect(lastPreview().period).toBe("30d");
    await act(async () => done());
    await waitFor(() => expect(screen.getByRole("button", { name: "Hari Ini" })).not.toHaveAttribute("aria-disabled"));
    expect(screen.getByRole("status")).toHaveTextContent("Tersalin");
  });
});

describe("SharePanel focus and keys", () => {
  it("ignores Escape while its tab is not the active one", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <TabActiveContext.Provider value={false}>
        <SharePanel onClose={onClose} />
      </TabActiveContext.Provider>,
    );
    await screen.findByRole("img", { name: /Pratinjau/ });
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("moves focus into the panel when it opens", async () => {
    await open();
    expect(screen.getByRole("region", { name: "Bagikan Pemakaian" })).toHaveFocus();
  });

  it("Escape and the close button close it", async () => {
    const user = userEvent.setup();
    const { onClose } = await open();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(within(screen.getByRole("region")).getByRole("button", { name: "Tutup" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("Escape that closes the open select does not also close the panel", async () => {
    const user = userEvent.setup();
    const { onClose } = await open();
    await user.click(screen.getByRole("combobox", { name: "Isi" }));
    await screen.findByRole("option", { name: "Codex" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("option", { name: "Codex" })).not.toBeInTheDocument());
    expect(onClose).not.toHaveBeenCalled();
  });
});
