import type { T } from "./i18n";

/** "Diperbarui 5 menit lalu" style label from an ISO timestamp. */
export function updatedLabel(iso: string | null, t: T, now: number = Date.now()): string {
  if (!iso) return t("updatedNever");
  const sec = Math.max(0, Math.floor((now - new Date(iso).getTime()) / 1000));
  if (sec < 60) return t("updatedNow");
  const min = Math.floor(sec / 60);
  if (min < 60) return t("updatedMin", { n: min });
  const hr = Math.floor(min / 60);
  if (hr < 24) return t("updatedHour", { n: hr });
  return t("updatedDay", { n: Math.floor(hr / 24) });
}
