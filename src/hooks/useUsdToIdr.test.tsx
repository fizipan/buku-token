import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "@testing-library/react";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { resetUsdToIdrCache, useUsdToIdr } from "./useUsdToIdr";

function Rate({ id }: { id: string }) {
  return <p data-testid={id}>{useUsdToIdr()}</p>;
}

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
});

describe("useUsdToIdr", () => {
  it("falls back to 16300, then shows the loaded rate", async () => {
    commands.get_settings = () => ({ usdToIdr: 16500 });
    render(<Rate id="a" />);
    expect(screen.getByTestId("a")).toHaveTextContent("16300");
    expect(await screen.findByText("16500")).toBeInTheDocument();
  });

  it("loads settings once for many consumers and across remounts", async () => {
    commands.get_settings = () => ({ usdToIdr: 16100 });
    const { unmount } = render(
      <>
        <Rate id="a" />
        <Rate id="b" />
      </>,
    );
    await screen.findAllByText("16100");
    unmount();
    render(<Rate id="c" />);
    expect(screen.getByTestId("c")).toHaveTextContent("16100");
    expect(invokeMock.mock.calls.filter(([c]) => c === "get_settings")).toHaveLength(1);
  });

  it("keeps the fallback when settings fail", async () => {
    commands.get_settings = () => {
      throw new Error("x");
    };
    render(<Rate id="a" />);
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled());
    expect(screen.getByTestId("a")).toHaveTextContent("16300");
  });

  it("refetches the rate on data-updated and updates rendered amounts", async () => {
    commands.get_settings = () => ({ usdToIdr: 16300 });
    render(<Rate id="a" />);
    await screen.findByText("16300");
    commands.get_settings = () => ({ usdToIdr: 17000 });
    await act(async () => {
      emit("data-updated");
    });
    expect(await screen.findByText("17000")).toBeInTheDocument();
  });
});
