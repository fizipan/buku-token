import { vi } from "vitest";

/** Shared fake of the Tauri bridge. Tests set `commands[name]` and call `emit(event, payload)`. */
export const commands: Record<string, (args?: unknown) => unknown> = {};
const listeners = new Map<string, Set<(e: { payload: unknown }) => void>>();

export const invokeMock = vi.fn(async (cmd: string, args?: unknown) => {
  const h = commands[cmd];
  if (!h) throw new Error(`no handler for ${cmd}`);
  return h(args);
});

export const listenMock = vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb);
  return () => listeners.get(event)!.delete(cb);
});

export function emit(event: string, payload: unknown = null) {
  listeners.get(event)?.forEach((cb) => cb({ payload }));
}

export function resetTauri() {
  for (const k of Object.keys(commands)) delete commands[k];
  listeners.clear();
  invokeMock.mockClear();
  listenMock.mockClear();
}
