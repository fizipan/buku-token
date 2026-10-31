import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import statusFixture from "../test/fixtures/scan-status.json";
import { commands, invokeMock, listenMock, resetTauri } from "../test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { Welcome } from "./Welcome";

beforeEach(() => {
  resetTauri();
  commands.save_settings = () => ({});
  commands.refresh = () => null;
  commands.scan_status = () => status();
});

const status = (sources: { agent: string; path: string; found: boolean }[] = []) => ({ ...statusFixture, sources });

describe("Welcome", () => {
  it("lists the agents whose folder exists, by name, and nothing else", async () => {
    commands.scan_status = () =>
      status([
        { agent: "cc", path: "/Users/dev/.claude/projects", found: true },
        { agent: "cx", path: "/Users/dev/.codex/sessions", found: false },
        { agent: "cx", path: "/Users/dev/.codex/archived_sessions", found: true },
        { agent: "oc", path: "/Users/dev/.local/share/opencode/opencode.db", found: false },
        { agent: "hm", path: "/Users/dev/.hermes/state.db", found: true },
      ]);
    render(<Welcome onDone={() => {}} />);
    const list = await screen.findByRole("list", { name: "Ditemukan di Mac Ini" });
    expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Claude Code", "Codex", "Hermes"]);
    expect(screen.queryByText("Belum Ada Agent Terdeteksi")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("/Users/dev");
  });

  it("says so when no agent folder exists, or when the scan status cannot be read", async () => {
    const { unmount } = render(<Welcome onDone={() => {}} />);
    expect(await screen.findByText("Belum Ada Agent Terdeteksi")).toBeInTheDocument();
    unmount();
    commands.scan_status = () => {
      throw new Error("putus");
    };
    render(<Welcome onDone={() => {}} />);
    expect(await screen.findByText("Belum Ada Agent Terdeteksi")).toBeInTheDocument();
  });

  it("announces the Keychain prompt only when Claude Code is on this Mac", async () => {
    render(<Welcome onDone={() => {}} />);
    await screen.findByText("Belum Ada Agent Terdeteksi");
    expect(screen.queryByText("macOS Akan Minta Izin")).not.toBeInTheDocument();
    expect(screen.queryByText(/Claude Code-credentials/)).not.toBeInTheDocument();
    expect(screen.getByText("Data Tetap Di Mac Ini")).toBeInTheDocument();
    expect(screen.getByText("Pakai Cursor Atau OpenRouter?")).toBeInTheDocument();
  });

  it("gives the three notes, honest about Keychain and cursor.com", async () => {
    commands.scan_status = () => status([{ agent: "cc", path: "/Users/dev/.claude/projects", found: true }]);
    render(<Welcome onDone={() => {}} />);
    await screen.findByRole("list", { name: "Ditemukan di Mac Ini" });
    expect(screen.getByText("Data Tetap Di Mac Ini")).toBeInTheDocument();
    expect(screen.getByText(/hanya diminta ke API pemakaian Anthropic/)).toBeInTheDocument();
    expect(screen.getByText("macOS Akan Minta Izin")).toBeInTheDocument();
    expect(screen.getByText(/Always Allow/)).toBeInTheDocument();
    expect(screen.getByText(/Claude Code-credentials/)).toBeInTheDocument();
    const optional = screen.getByText("Pakai Cursor Atau OpenRouter?").parentElement!;
    expect(optional).toHaveTextContent(/Pengaturan/);
    expect(optional).toHaveTextContent(/cookie sesi Cursor hanya dikirim ke cursor\.com/);
    expect(optional).toHaveTextContent(/API key OpenRouter hanya ke openrouter\.ai/);
    expect(optional).toHaveTextContent(/setelah kamu menghubungkannya/);
  });

  it("Mulai saves onboarded first, then refreshes, then reports done", async () => {
    const onDone = vi.fn();
    render(<Welcome onDone={onDone} />);
    await userEvent.click(screen.getByRole("button", { name: "Mulai" }));
    await waitFor(() => expect(onDone).toHaveBeenCalledTimes(1));
    const calls = invokeMock.mock.calls.map((c) => c[0]).filter((c) => c === "save_settings" || c === "refresh");
    expect(calls).toEqual(["save_settings", "refresh"]);
    expect(invokeMock).toHaveBeenCalledWith("save_settings", { settings: { onboarded: true } });
  });

  it("shows an error and stays when saving fails", async () => {
    commands.save_settings = () => {
      throw new Error("disk penuh");
    };
    const onDone = vi.fn();
    render(<Welcome onDone={onDone} />);
    await userEvent.click(screen.getByRole("button", { name: "Mulai" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("disk penuh");
    expect(onDone).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Mulai" })).toBeEnabled();
  });
});
