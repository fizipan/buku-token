import antigravity from "@/assets/logos/antigravity.svg?raw";
import claude from "@/assets/logos/claude.svg?raw";
import cursor from "@/assets/logos/cursor.svg?raw";
import kimi from "@/assets/logos/kimi.svg?raw";
import minimax from "@/assets/logos/minimax.svg?raw";
import nousresearch from "@/assets/logos/nousresearch.svg?raw";
import openai from "@/assets/logos/openai.svg?raw";
import openrouter from "@/assets/logos/openrouter.svg?raw";
import type { LogoId } from "@/lib/agents";

const RAW: Record<LogoId, string> = { claude, openai, kimi, minimax, nousresearch, cursor, openrouter, antigravity };

/** Turns a full `<svg>` file into a `<symbol>`: keeps viewBox and the inner paths, drops `<title>`. */
function toSymbol(id: LogoId, raw: string): string {
  const viewBox = /viewBox="([^"]+)"/.exec(raw)?.[1] ?? "0 0 24 24";
  const inner = raw
    .replace(/<title>[\s\S]*?<\/title>/, "")
    .replace(/^[\s\S]*?<svg[^>]*>/, "")
    .replace(/<\/svg>\s*$/, "");
  return `<symbol id="lg-${id}" viewBox="${viewBox}" fill="currentColor" fill-rule="evenodd">${inner}</symbol>`;
}

const SPRITE = (Object.keys(RAW) as LogoId[]).map((id) => toSymbol(id, RAW[id])).join("");

/** Render once at the app root. */
export function LogoSprite() {
  return (
    <svg
      aria-hidden="true"
      width="0"
      height="0"
      style={{ position: "absolute" }}
      dangerouslySetInnerHTML={{ __html: SPRITE }}
    />
  );
}

export function Logo({ id, size = 14, className }: { id: LogoId; size?: number; className?: string }) {
  return (
    <svg aria-hidden="true" width={size} height={size} className={className} fill="currentColor">
      <use href={`#lg-${id}`} />
    </svg>
  );
}
