import { Info } from "lucide-react";
import type { ReactNode } from "react";

/** Muted insight note with an info icon. Bold parts go in <b>. */
export function Callout({ children }: { children: ReactNode }) {
  return (
    <div role="note" className="flex items-start gap-2.5 rounded-lg bg-muted px-3 py-2.5 text-[13px] leading-snug">
      <Info aria-hidden className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      <p className="[&_b]:font-semibold">{children}</p>
    </div>
  );
}
