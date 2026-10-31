import { Badge } from "@/components/ui/badge";
import { useT } from "@/lib/i18n";

/** An amount with tokens but no known price: a dash and a small "Harga Belum Ada". */
export function Unpriced() {
  const t = useT();
  return (
    <span className="inline-flex items-center gap-1.5">
      <Badge variant="outline">{t("noPrice")}</Badge>
      <span>–</span>
    </span>
  );
}
