import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { getModels } from "../lib/api";
import { useTabData } from "../hooks/useTabData";
import { DataView } from "./DataView";

function Probe() {
  const state = useTabData(() => getModels("7d"), []);
  return (
    <DataView state={state}>
      {(d) => <p>rows:{(d as unknown as { n: number }).n}</p>}
    </DataView>
  );
}

beforeEach(() => resetTauri());

describe("useTabData + DataView", () => {
  it("shows_loading_only_on_first_load", async () => {
    commands.get_models = () => ({ n: 1 });
    render(<Probe />);
    expect(screen.getByText("Memuat…")).toBeInTheDocument();
    expect(await screen.findByText("rows:1")).toBeInTheDocument();
  });

  it("refresh_keeps_previous_render", async () => {
    commands.get_models = () => ({ n: 1 });
    render(<Probe />);
    await screen.findByText("rows:1");

    let release!: (v: unknown) => void;
    commands.get_models = () => new Promise((r) => (release = r));
    act(() => emit("data-updated"));
    const old = await screen.findByText("rows:1");
    expect(old).toBeInTheDocument();
    expect(screen.queryByText("Memuat…")).not.toBeInTheDocument();
    expect(document.querySelector('[data-slot="skeleton"]')).toBeNull();
    expect(screen.getByTestId("data-view")).toHaveStyle({ opacity: "0.6" });

    await act(async () => release({ n: 2 }));
    expect(screen.getByText("rows:2")).toBeInTheDocument();
    expect(screen.getByTestId("data-view")).toHaveStyle({ opacity: "1" });
  });

  it("refetches_on_data_updated", async () => {
    commands.get_models = () => ({ n: 1 });
    render(<Probe />);
    await screen.findByText("rows:1");
    commands.get_models = () => ({ n: 7 });
    act(() => emit("data-updated"));
    expect(await screen.findByText("rows:7")).toBeInTheDocument();
  });

  it("shows_error_with_retry", async () => {
    const user = userEvent.setup();
    commands.get_models = () => {
      throw "db terkunci";
    };
    render(<Probe />);
    expect(await screen.findByText(/db terkunci/)).toBeInTheDocument();
    commands.get_models = () => ({ n: 3 });
    await user.click(screen.getByRole("button", { name: "Coba Lagi" }));
    expect(await screen.findByText("rows:3")).toBeInTheDocument();
  });
});
