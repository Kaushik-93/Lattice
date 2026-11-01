"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useState } from "react";

import { isSignedOutError } from "@/components/shell/auth-gate";
import { ErrorState } from "@/components/shell/page-skeleton";
import { SourceBadge, StatusPill, kindInfo } from "@/components/sources/source-meta";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { formatCount, timeAgo } from "@/lib/format";
import { useDeleteSource, useProject, useRefreshSource, useSource } from "@/lib/queries";
import { cn } from "@/lib/utils";

import { TablePanel } from "./table-panel";

export function SourceExplorer() {
  const { projectId, sourceId } = useParams<{ projectId: string; sourceId: string }>();
  const router = useRouter();
  const source = useSource(projectId, sourceId);
  const project = useProject(projectId);
  const refresh = useRefreshSource(projectId);
  const remove = useDeleteSource(projectId);
  const [selected, setSelected] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (source.isPending || isSignedOutError(source.error)) return <ExplorerSkeleton />;
  if (source.error) {
    return (
      <ErrorState
        title="Source not found"
        message="It doesn't exist in this project, or you don't have access to it."
        action={
          <Link
            href={`/projects/${projectId}/sources`}
            className="text-sm font-medium no-underline"
          >
            ← Sources
          </Link>
        }
      />
    );
  }

  const { source: s, tables } = source.data;
  const table = tables.find((t) => t.id === selected) ?? tables[0];
  const role = project.data?.project.my_role;
  const isAdmin = role === "admin";
  const canProfile = role === "admin" || role === "editor";

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex items-center gap-4">
          <SourceBadge source={s} size={48} />
          <div className="flex flex-col gap-1">
            <span className="text-[13px] text-subtle-foreground">
              <Link
                href="/projects"
                className="text-subtle-foreground no-underline hover:text-foreground"
              >
                Projects
              </Link>{" "}
              /{" "}
              <Link
                href={`/projects/${projectId}`}
                className="text-subtle-foreground no-underline hover:text-foreground"
              >
                {project.data?.project.name ?? "…"}
              </Link>{" "}
              /{" "}
              <Link
                href={`/projects/${projectId}/sources`}
                className="text-subtle-foreground no-underline hover:text-foreground"
              >
                Sources
              </Link>
            </span>
            <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">{s.name}</h1>
            <p className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
              <span>
                {kindInfo(s).type} · {s.detail} · synced {timeAgo(s.synced_at)}
              </span>
              <StatusPill status={s.status} title={s.status_detail} />
            </p>
            {s.status === "error" && s.status_detail && (
              <p className="text-sm text-warning">{s.status_detail}</p>
            )}
          </div>
        </div>
        {isAdmin && (
          <div className="flex flex-wrap gap-2.5">
            <Button
              variant="outline"
              disabled={refresh.isPending}
              onClick={() => refresh.mutate(s.id)}
            >
              {refresh.isPending ? "Refreshing…" : "Refresh schema"}
            </Button>
            {confirmDelete ? (
              <Button
                variant="destructive"
                disabled={remove.isPending}
                onClick={() =>
                  remove.mutate(s.id, {
                    onSuccess: () => router.replace(`/projects/${projectId}/sources`),
                  })
                }
              >
                {remove.isPending ? "Removing…" : "Confirm remove"}
              </Button>
            ) : (
              <Button variant="outline" onClick={() => setConfirmDelete(true)}>
                Remove source
              </Button>
            )}
          </div>
        )}
      </header>
      {(refresh.error ?? remove.error) && (
        <p role="alert" className="text-sm text-destructive">
          {(refresh.error ?? remove.error)?.message}
        </p>
      )}

      <div className="grid grid-cols-[minmax(0,240px)_minmax(0,1fr)] items-start gap-5 max-lg:grid-cols-1">
        <nav
          aria-label="Tables"
          className="flex flex-col gap-0.5 rounded-xl border border-border bg-card p-2 lg:sticky lg:top-6"
        >
          <span className="px-2.5 pt-1 pb-2 text-[11px] font-semibold tracking-[0.06em] text-subtle-foreground uppercase">
            Tables · {tables.length}
          </span>
          <div className="flex max-h-[60vh] flex-col gap-0.5 overflow-y-auto">
            {tables.map((t) => (
              <button
                key={t.id}
                type="button"
                onClick={() => setSelected(t.id)}
                aria-current={t.id === table?.id ? "true" : undefined}
                className={cn(
                  "flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-left text-sm",
                  t.id === table?.id
                    ? "bg-accent font-semibold text-accent-foreground"
                    : "hover:bg-secondary",
                )}
              >
                <span className="truncate">
                  {t.schema_name.startsWith("s_") ? t.name : `${t.schema_name}.${t.name}`}
                </span>
                <span className="shrink-0 font-mono text-[11px] text-subtle-foreground">
                  {formatCount(t.row_count)}
                </span>
              </button>
            ))}
            {tables.length === 0 && (
              <p className="px-2.5 py-2 text-sm text-muted-foreground">No tables found.</p>
            )}
          </div>
        </nav>
        {table ? (
          <TablePanel key={table.id} projectId={projectId} table={table} canProfile={canProfile} />
        ) : (
          <ErrorState title="Nothing to explore" message="This source has no tables yet." />
        )}
      </div>
    </>
  );
}

function ExplorerSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy="true" aria-label="Loading source">
      <div className="flex items-center gap-4">
        <Skeleton className="size-12 rounded-[10px]" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-3 w-48" />
          <Skeleton className="h-7 w-72" />
        </div>
      </div>
      <div className="grid grid-cols-[240px_1fr] gap-5">
        <Skeleton className="h-72 rounded-xl" />
        <Skeleton className="h-72 rounded-xl" />
      </div>
    </div>
  );
}
