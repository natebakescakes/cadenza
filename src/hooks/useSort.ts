import { useMemo, useState } from "react";

export type SortDir = "asc" | "desc";

/** Generic client-side sort state + sorted rows for tables. */
export function useSort<Row>(
  rows: Row[],
  initialKey: keyof Row & string,
  initialDir: SortDir = "desc",
) {
  const [sortKey, setSortKey] = useState<keyof Row & string>(initialKey);
  const [sortDir, setSortDir] = useState<SortDir>(initialDir);

  const toggle = (key: string) => {
    const k = key as keyof Row & string;
    if (k === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(k);
      setSortDir("desc");
    }
  };

  const sorted = useMemo(() => {
    const copy = [...rows];
    copy.sort((a, b) => {
      const av = a[sortKey];
      const bv = b[sortKey];
      let cmp = 0;
      if (typeof av === "number" && typeof bv === "number") cmp = av - bv;
      else cmp = String(av).localeCompare(String(bv));
      return sortDir === "asc" ? cmp : -cmp;
    });
    return copy;
  }, [rows, sortKey, sortDir]);

  return { sorted, sortKey, sortDir, toggle };
}
