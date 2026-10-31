import { createContext, useContext, useEffect, useRef, useState, type DependencyList } from "react";
import { onDataUpdated } from "../lib/api";

export interface TabData<T> {
  /** Last successful result; kept while a refetch is in flight. */
  data: T | null;
  error: string | null;
  /** True while refetching with data already on screen. */
  refreshing: boolean;
  reload: () => void;
}

/** False while this tab's panel is hidden. Tabs stay mounted so they keep their data and filters. */
export const TabActiveContext = createContext(true);

/**
 * Fetches tab data, refetches on `data-updated`, and whenever `deps` change.
 * Previous data stays visible during refetches (no skeleton). A hidden tab ignores `data-updated`
 * and catches up once when it is shown again.
 */
export function useTabData<T>(fetch: () => Promise<T>, deps: DependencyList = []): TabData<T> {
  const active = useContext(TabActiveContext);
  const activeRef = useRef(active);
  activeRef.current = active;
  const missedUpdate = useRef(false);
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [nonce, setNonce] = useState(0);
  const fetchRef = useRef(fetch);
  fetchRef.current = fetch;

  useEffect(() => {
    let live = true;
    setRefreshing(true);
    fetchRef
      .current()
      .then((d) => {
        if (!live) return;
        setData(d);
        setError(null);
      })
      .catch((e) => live && setError(String(e)))
      .finally(() => live && setRefreshing(false));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);

  useEffect(() => {
    const un = onDataUpdated(() => {
      if (activeRef.current) setNonce((n) => n + 1);
      else missedUpdate.current = true;
    });
    return () => void un.then((f) => f());
  }, []);

  useEffect(() => {
    if (active && missedUpdate.current) {
      missedUpdate.current = false;
      setNonce((n) => n + 1);
    }
  }, [active]);

  return { data, error, refreshing, reload: () => setNonce((n) => n + 1) };
}
