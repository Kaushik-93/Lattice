import type { SourceKind, SourceStatus, SourceSummary } from "@lattice/shared-types";

import { cn } from "@/lib/utils";

const KIND: Record<SourceKind, { badge: string; type: string }> = {
  postgres: { badge: "PG", type: "PostgreSQL" },
  mysql: { badge: "MY", type: "MySQL" },
  sqlite: { badge: "SQ", type: "SQLite" },
  file: { badge: "FILE", type: "File" },
  demo: { badge: "DEMO", type: "Demo dataset" },
};

const FILE_BADGES: Record<string, { badge: string; type: string }> = {
  csv: { badge: "CSV", type: "CSV" },
  tsv: { badge: "TSV", type: "TSV" },
  json: { badge: "JSON", type: "JSON" },
  ndjson: { badge: "JSON", type: "NDJSON" },
  jsonl: { badge: "JSON", type: "NDJSON" },
  parquet: { badge: "PQ", type: "Parquet" },
  xlsx: { badge: "XLS", type: "Excel" },
};

export function kindInfo(source: Pick<SourceSummary, "kind" | "name">): {
  badge: string;
  type: string;
} {
  if (source.kind === "file") {
    const ext = source.name.split(".").pop()?.toLowerCase() ?? "";
    return FILE_BADGES[ext] ?? KIND.file;
  }
  return KIND[source.kind];
}

export function SourceBadge({
  source,
  size = 34,
}: {
  source: Pick<SourceSummary, "kind" | "name">;
  size?: number;
}) {
  return (
    <span
      aria-hidden="true"
      className="flex flex-none items-center justify-center rounded-[9px] bg-secondary font-mono text-[11px] font-medium text-secondary-foreground"
      style={{ width: size, height: size }}
    >
      {kindInfo(source).badge}
    </span>
  );
}

const STATUS: Record<SourceStatus, { label: string; className: string; dot: string }> = {
  ready: { label: "Healthy", className: "bg-success-surface text-success", dot: "#0F766E" },
  syncing: { label: "Syncing", className: "bg-accent text-accent-foreground", dot: "#4338CA" },
  error: { label: "Needs attention", className: "bg-warning-surface text-warning", dot: "#B45309" },
};

export function StatusPill({ status, title }: { status: SourceStatus; title?: string | null }) {
  const s = STATUS[status];
  return (
    <span
      title={title ?? undefined}
      className={cn("inline-flex rounded-full px-[9px] py-[3px] text-xs font-medium", s.className)}
    >
      {s.label}
    </span>
  );
}

export function statusDot(status: SourceStatus): string {
  return STATUS[status].dot;
}

export function statusLabel(status: SourceStatus): string {
  return STATUS[status].label;
}
