import type { Key } from "./i18n/id";

export const TAB_IDS = ["ringkasan", "mutasi", "model", "project", "riwayat"] as const;
export type TabId = (typeof TAB_IDS)[number];

/** Dictionary key of each tab's label. */
export const TAB_LABEL: Record<TabId, Key> = {
  ringkasan: "tabRingkasan",
  mutasi: "tabMutasi",
  model: "tabModel",
  project: "tabProject",
  riwayat: "tabRiwayat",
};

export function toTabId(v: string | undefined): TabId {
  return (TAB_IDS as readonly string[]).includes(v ?? "") ? (v as TabId) : "ringkasan";
}
