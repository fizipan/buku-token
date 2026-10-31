import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";

/**
 * A destructive button with an inline question first. While `busy` it stays focusable (`aria-disabled`, the click
 * is ignored), so keyboard focus is never lost. Batal puts focus back on the button.
 */
export function ConfirmButton({
  label,
  ariaLabel,
  question,
  yes,
  busy = false,
  onConfirm,
}: {
  label: string;
  ariaLabel?: string;
  question: string;
  yes: string;
  busy?: boolean;
  onConfirm: () => void;
}) {
  const t = useT();
  const [asking, setAsking] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const wasAsking = useRef(false);
  useEffect(() => {
    if (wasAsking.current && !asking) trigger.current?.focus();
    wasAsking.current = asking;
  }, [asking]);

  if (asking) {
    return (
      <span role="group" aria-label={question} className="flex flex-wrap items-center justify-end gap-2">
        <span className="text-xs text-muted-foreground">{question}</span>
        <Button
          variant="outline"
          size="sm"
          className="text-destructive"
          aria-disabled={busy || undefined}
          onClick={() => {
            if (busy) return;
            setAsking(false);
            onConfirm();
          }}
        >
          {yes}
        </Button>
        <Button variant="ghost" size="sm" autoFocus onClick={() => setAsking(false)}>
          {t("cancel")}
        </Button>
      </span>
    );
  }
  return (
    <Button ref={trigger} variant="outline" size="sm" aria-label={ariaLabel} aria-disabled={busy || undefined} onClick={() => !busy && setAsking(true)}>
      {label}
    </Button>
  );
}
