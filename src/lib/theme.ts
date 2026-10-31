import type { ThemePref } from "./api";

let pref: ThemePref = "system";
let systemDark = false;

/** `<html class="dark">` is what shadcn's dark tokens listen to. */
const apply = () => document.documentElement.classList.toggle("dark", pref === "dark" || (pref === "system" && systemDark));

/** Sistem follows macOS, Terang and Gelap override it. Takes effect at once. */
export function setTheme(next: ThemePref): void {
  pref = next;
  apply();
}

/** Tracks macOS light/dark for the Sistem choice. Returns a cleanup. */
export function syncColorScheme(): () => void {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const onChange = (e: { matches: boolean }) => {
    systemDark = e.matches;
    apply();
  };
  onChange(mq);
  mq.addEventListener("change", onChange);
  return () => mq.removeEventListener("change", onChange);
}
