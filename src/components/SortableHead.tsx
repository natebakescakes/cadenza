import type { ReactNode } from "react";
import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react";
import { TableHead } from "@/components/ui/table";
import { cn } from "@/lib/utils";
import type { SortDir } from "@/hooks/useSort";

export interface SortableHeadProps {
  columnKey: string;
  sortKey: string;
  sortDir: SortDir;
  onSort: (key: string) => void;
  align?: "left" | "right";
  children: ReactNode;
  className?: string;
}

/** Sortable header cell for the shadcn Table primitive. */
export function SortableHead({
  columnKey,
  sortKey,
  sortDir,
  onSort,
  align = "left",
  children,
  className,
}: SortableHeadProps) {
  const active = sortKey === columnKey;
  return (
    <TableHead
      className={cn(
        "text-xs font-medium tracking-wider text-muted-foreground/70 uppercase",
        align === "right" && "text-right",
        className,
      )}
    >
      <button
        type="button"
        onClick={() => onSort(columnKey)}
        className={cn(
          "inline-flex items-center gap-1.5 outline-none transition-colors hover:text-foreground focus-visible:text-gold",
          align === "right" && "w-full flex-row-reverse",
          active && "text-foreground",
        )}
      >
        {children}
        {active ? (
          sortDir === "asc" ? (
            <ArrowUp className="size-3 text-gold" />
          ) : (
            <ArrowDown className="size-3 text-gold" />
          )
        ) : (
          <ChevronsUpDown className="size-3 opacity-40" />
        )}
      </button>
    </TableHead>
  );
}
