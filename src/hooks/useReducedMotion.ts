import { useSyncExternalStore } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";
const mql = () => (typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(QUERY) : null);

function subscribe(cb: () => void) {
  const m = mql();
  m?.addEventListener("change", cb);
  return () => m?.removeEventListener("change", cb);
}

/** True when the user asked the OS for reduced motion; charts then skip their draw animations. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, () => mql()?.matches ?? false, () => false);
}
