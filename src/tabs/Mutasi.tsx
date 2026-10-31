import { useEffect, useRef, useState, type RefObject } from "react";
import { AgentTile } from "@/components/AgentTile";
import { DataView } from "@/components/DataView";
import { Unpriced } from "@/components/Unpriced";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useTabData } from "@/hooks/useTabData";
import { useDisplayPrefs, useUsdToIdr } from "@/hooks/useUsdToIdr";
import { AGENT_NAME, STACK_ORDER, modelLabel } from "@/lib/agents";
import { getSessions, type AgentKey, type DayGroup, type ProjectRef, type SessionPage, type SessionRow } from "@/lib/api";
import { isUnpricedOnly } from "@/lib/format";
import { useI18n, useLang } from "@/lib/i18n";

const ALL = "__all__";
const NONE = "__none__"; // Radix Select cannot use "" as an item value; maps to path "" (Tanpa Project)
const PAGE_DAYS = 3;

const toProjectPath = (v: string) => (v === ALL ? null : v === NONE ? "" : v);
const toProjectValue = (p: ProjectRef) => (p.path === "" ? NONE : p.path);

export function Mutasi() {
  const [agent, setAgent] = useState<string>(ALL);
  const [project, setProject] = useState<string>(ALL);
  const lang = useLang();
  // Only agents that are switched on can be chosen; one that gets switched off goes back to Semua Agent.
  const { enabledAgents } = useDisplayPrefs();
  const offered = STACK_ORDER.filter((a) => enabledAgents[a] !== false);
  useEffect(() => {
    if (agent !== ALL && enabledAgents[agent as AgentKey] === false) setAgent(ALL);
  }, [agent, enabledAgents]);
  const filter = { agent: agent === ALL ? null : (agent as AgentKey), projectPath: toProjectPath(project), days: PAGE_DAYS };
  // Days on screen: a refresh refetches all of them, so a new day at the top cannot open a gap.
  const shownDays = useRef(PAGE_DAYS);
  const state = useTabData(() => getSessions({ ...filter, days: shownDays.current }), [agent, project]);
  const onFilter = (set: (v: string) => void) => (v: string) => {
    shownDays.current = PAGE_DAYS;
    set(v);
  };

  return (
    <DataView state={state}>
      {(first) => (
        <Body
          key={`${agent}|${project}|${lang}`}
          first={first}
          filter={filter}
          shownDays={shownDays}
          agent={agent}
          offered={offered}
          project={project}
          onAgent={onFilter(setAgent)}
          onProject={onFilter(setProject)}
        />
      )}
    </DataView>
  );
}

interface BodyProps {
  first: SessionPage;
  filter: Parameters<typeof getSessions>[0];
  shownDays: RefObject<number>;
  agent: string;
  offered: readonly AgentKey[];
  project: string;
  onAgent: (v: string) => void;
  onProject: (v: string) => void;
}

function Body({ first, filter, shownDays, agent, offered, project, onAgent, onProject }: BodyProps) {
  const rate = useUsdToIdr();
  const { t, f } = useI18n();
  const [older, setOlder] = useState<DayGroup[]>([]);
  const [olderHasMore, setOlderHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Only the latest request of this filter may land; `Body` remounts when the filter changes.
  const request = useRef(0);
  useEffect(
    () => () => {
      request.current++;
    },
    [],
  );

  // The first page is the fresher one: it wins for every day it reaches.
  const firstOldest = first.days.slice(-1)[0]?.date;
  const tail = firstOldest ? older.filter((d) => d.date < firstOldest) : older;
  const days = [...first.days, ...tail];
  const hasMore = tail.length > 0 ? olderHasMore : first.hasMore;
  const oldest = days.slice(-1)[0]?.date ?? null;

  const loadMore = async () => {
    if (!oldest) return;
    const mine = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const next = await getSessions({ ...filter, before: oldest });
      if (mine !== request.current) return;
      setOlder((o) => [...o, ...next.days]);
      setOlderHasMore(next.hasMore);
      shownDays.current += next.days.length;
    } catch (e) {
      if (mine === request.current) setError(String(e));
    } finally {
      if (mine === request.current) setLoading(false);
    }
  };

  return (
    <div className="px-5 pb-5 pt-[18px]">
      <div className="mb-3 flex gap-2">
        <Select value={agent} onValueChange={onAgent}>
          <SelectTrigger aria-label={t("filterAgent")} className="min-w-0 flex-1 text-[13px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("allAgents")}</SelectItem>
            {offered.map((a) => (
              <SelectItem key={a} value={a}>
                <AgentTile agent={a} size={18} />
                {AGENT_NAME[a]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={project} onValueChange={onProject}>
          <SelectTrigger aria-label={t("filterProject")} className="min-w-0 flex-1 text-[13px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="w-[260px]" align="end">
            <SelectItem value={ALL}>{t("allProjects")}</SelectItem>
            {first.projects.map((p) => (
              <SelectItem key={toProjectValue(p)} value={toProjectValue(p)}>
                <span className="truncate">{p.name}</span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {days.length === 0 && <p className="py-8 text-center text-[13px] text-muted-foreground">{t("noSessions")}</p>}

      {days.map((d) => (
        <section key={d.date} className="mt-4 first:mt-0">
          <h3 className="mb-1 flex items-baseline justify-between gap-3 text-[13px] font-semibold">
            <span>{d.label}</span>
            <span className="font-normal tabular-nums text-muted-foreground">
              {isUnpricedOnly(d.costUsd, d.unpricedTokens) ? <Unpriced /> : `${f.usd(d.costUsd)} · ${f.idr(d.costUsd, rate)}`}
            </span>
          </h3>
          <ul>
            {d.sessions.map((s) => (
              <SessionItem key={s.sessionId} s={s} />
            ))}
          </ul>
        </section>
      ))}

      {error && (
        <p role="alert" className="mt-3 text-center text-[12.5px] text-destructive">
          {t("loadFailedShort", { error })}
        </p>
      )}
      {hasMore && (
        <Button variant="outline" className="mt-4 w-full" disabled={loading} onClick={() => void loadMore()}>
          {t("loadEarlier", { n: PAGE_DAYS })}
        </Button>
      )}
    </div>
  );
}

function SessionItem({ s }: { s: SessionRow }) {
  const { t, f } = useI18n();
  return (
    <li className="flex items-center gap-3 py-2">
      <AgentTile agent={s.agent} size={24} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13.5px] font-medium">{s.project.name}</div>
        <div className="truncate text-xs tabular-nums text-muted-foreground">
          {`${f.clock(s.startedAt)} · ${modelLabel(s.mainModel)} · ${f.duration(s.durationMin)}`}
        </div>
      </div>
      <div className="flex shrink-0 flex-col items-end gap-0.5 tabular-nums">
        <div className="flex items-center gap-1.5">
          {s.costUsd === null && <Badge variant="outline">{t("noPrice")}</Badge>}
          <span className="text-[13.5px] font-semibold">{f.usd(s.costUsd)}</span>
        </div>
        <span className="text-xs text-muted-foreground">{t("tokenCount", { n: f.tokens(s.tokens) })}</span>
      </div>
    </li>
  );
}
