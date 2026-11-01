"use client";

import type { SourceDetail } from "@lattice/shared-types";
import { Upload } from "lucide-react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useRef, useState, type DragEvent } from "react";

import { isSignedOutError } from "@/components/shell/auth-gate";
import { ErrorState } from "@/components/shell/page-skeleton";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { compactCount, formatCount, pluralize, timeAgo } from "@/lib/format";
import { useProject, useSources, useSourcesInvalidation } from "@/lib/queries";
import { ACCEPTED_FILES, uploadSource } from "@/lib/upload";
import { cn } from "@/lib/utils";

import { DatabaseDialog, type DatabaseKind } from "./database-dialog";
import { SourceBadge, StatusPill, kindInfo } from "./source-meta";

/** The design's tiles. `null` actions are later phases. */
const TILES: { code: string; label: string; action: DatabaseKind | "file" | null }[] = [
  { code: "PG", label: "PostgreSQL", action: "postgres" },
  { code: "SQL", label: "SQL Server", action: null },
  { code: "SF", label: "Snowflake", action: null },
  { code: "BQ", label: "BigQuery", action: null },
  { code: "MY", label: "MySQL", action: "mysql" },
  { code: "API", label: "REST API", action: null },
  { code: "CSV", label: "CSV", action: "file" },
  { code: "XLS", label: "Excel", action: "file" },
];

type UploadState =
  | { phase: "idle" }
  | { phase: "uploading"; name: string; fraction: number }
  | { phase: "processing"; name: string }
  | { phase: "done"; detail: SourceDetail }
  | { phase: "failed"; name: string; message: string };

export function SourcesView() {
  const { projectId } = useParams<{ projectId: string }>();
  const router = useRouter();
  const project = useProject(projectId);
  const sources = useSources(projectId);
  const changed = useSourcesInvalidation(projectId);
  const fileInput = useRef<HTMLInputElement>(null);
  const [dialog, setDialog] = useState<DatabaseKind | null>(null);
  const [upload, setUpload] = useState<UploadState>({ phase: "idle" });
  const [dragging, setDragging] = useState(false);

  const isAdmin = project.data?.project.my_role === "admin";
  const syncing = sources.data?.filter((s) => s.status === "syncing").length ?? 0;

  async function startUpload(file: File) {
    setUpload({ phase: "uploading", name: file.name, fraction: 0 });
    try {
      const detail = await uploadSource(projectId, file, (fraction) =>
        setUpload(
          fraction >= 1
            ? { phase: "processing", name: file.name }
            : { phase: "uploading", name: file.name, fraction },
        ),
      );
      changed(detail);
      setUpload({ phase: "done", detail });
    } catch (err) {
      setUpload({
        phase: "failed",
        name: file.name,
        message: err instanceof Error ? err.message : "Upload failed.",
      });
      changed();
    }
  }

  function onDrop(event: DragEvent<HTMLLabelElement>) {
    event.preventDefault();
    setDragging(false);
    const file = event.dataTransfer.files[0];
    if (file && isAdmin) void startUpload(file);
  }

  if (project.error && !isSignedOutError(project.error)) {
    return (
      <ErrorState
        title="Project not found"
        message="It doesn't exist, or you don't have access to it."
        action={
          <Link href="/projects" className="text-sm font-medium no-underline">
            ← All projects
          </Link>
        }
      />
    );
  }

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
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
            / Sources
          </span>
          <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">Sources</h1>
          <p className="max-w-[640px] text-[15px] text-muted-foreground">
            The connectors that feed this project. Each project keeps its own sources and
            credentials; Lattice profiles every new one so you can see what&apos;s inside before you
            build on it.
          </p>
        </div>
        {isAdmin && <Button onClick={() => fileInput.current?.click()}>+ Add source</Button>}
      </header>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,420px),1fr))] items-start gap-5">
        <section
          aria-labelledby="connected-h"
          className="overflow-hidden rounded-xl border border-border bg-card"
        >
          <div className="flex items-center justify-between border-b border-border px-5 py-[18px]">
            <h2 id="connected-h" className="text-base font-semibold">
              Connected sources
            </h2>
            {sources.data && (
              <span className="text-[13px] text-subtle-foreground">
                {pluralize(sources.data.length, "source")}
                {syncing > 0 && ` · ${syncing} syncing`}
              </span>
            )}
          </div>
          <div className="overflow-x-auto">
            <table className="w-full min-w-[560px] border-collapse text-sm">
              <thead>
                <tr className="text-left text-xs tracking-[0.04em] text-subtle-foreground uppercase">
                  <th className="px-5 py-2.5 font-medium">Source</th>
                  <th className="px-3 py-2.5 font-medium">Type</th>
                  <th className="px-3 py-2.5 font-medium">Status</th>
                  <th className="py-2.5 pr-5 pl-3 text-right font-medium">Last sync</th>
                </tr>
              </thead>
              <tbody>
                {sources.isPending || isSignedOutError(sources.error)
                  ? [0, 1, 2].map((i) => (
                      <tr key={i} className="border-t border-border-subtle">
                        <td className="px-5 py-3.5" colSpan={4}>
                          <Skeleton className="h-8 w-full" />
                        </td>
                      </tr>
                    ))
                  : (sources.data ?? []).map((s) => (
                      <tr
                        key={s.id}
                        onClick={() => router.push(`/projects/${projectId}/sources/${s.id}`)}
                        className="cursor-pointer border-t border-border-subtle hover:bg-[#fafbfc]"
                      >
                        <td className="px-5 py-3.5">
                          <div className="flex items-center gap-3">
                            <SourceBadge source={s} />
                            <div className="flex min-w-0 flex-col gap-0.5">
                              <Link
                                href={`/projects/${projectId}/sources/${s.id}`}
                                className="truncate font-medium text-foreground no-underline hover:text-primary"
                              >
                                {s.name}
                              </Link>
                              <span
                                className="truncate text-xs text-subtle-foreground"
                                title={s.detail}
                              >
                                {pluralize(s.table_count, "table")}
                                {s.row_count !== null && ` · ${compactCount(s.row_count)} rows`}
                              </span>
                            </div>
                          </div>
                        </td>
                        <td className="px-3 py-3.5 text-secondary-foreground">
                          {kindInfo(s).type}
                        </td>
                        <td className="px-3 py-3.5">
                          <StatusPill status={s.status} title={s.status_detail} />
                        </td>
                        <td className="py-3.5 pr-5 pl-3 text-right font-mono text-[13px] text-muted-foreground">
                          {timeAgo(s.synced_at)}
                        </td>
                      </tr>
                    ))}
                {sources.data?.length === 0 && (
                  <tr className="border-t border-border-subtle">
                    <td colSpan={4} className="px-5 py-8 text-center text-sm text-muted-foreground">
                      No sources yet.{" "}
                      {isAdmin ? "Add one on the right." : "Ask a project admin to add one."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </section>

        <div className="flex flex-col gap-5">
          <section
            aria-labelledby="add-h"
            className="flex flex-col gap-4 rounded-xl border border-border bg-card p-5"
          >
            <h2 id="add-h" className="text-base font-semibold">
              Add a source
            </h2>
            {!isAdmin && project.data && (
              <p className="text-sm text-muted-foreground">Only project admins can add sources.</p>
            )}
            <div className="grid grid-cols-4 gap-2.5">
              {TILES.map((tile) => {
                const enabled = isAdmin && tile.action !== null;
                const button = (
                  <button
                    type="button"
                    disabled={!enabled}
                    onClick={() =>
                      tile.action === "file"
                        ? fileInput.current?.click()
                        : tile.action && setDialog(tile.action)
                    }
                    className="flex min-h-[76px] w-full cursor-pointer flex-col items-center justify-center gap-1.5 rounded-[10px] border border-border bg-card p-2 text-foreground hover:border-[#a5abf0] disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:border-border"
                  >
                    <span className="rounded-md bg-accent px-[7px] py-[3px] font-mono text-xs font-medium text-accent-foreground">
                      {tile.code}
                    </span>
                    <span className="text-[13px] font-medium">{tile.label}</span>
                  </button>
                );
                return tile.action === null ? (
                  <Tooltip key={tile.code}>
                    <TooltipTrigger asChild>
                      <span tabIndex={0} className="flex rounded-[10px]">
                        {button}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>Coming with the warehouse connectors (phase 5)</TooltipContent>
                  </Tooltip>
                ) : (
                  <div key={tile.code}>{button}</div>
                );
              })}
            </div>
            <label
              htmlFor="upload"
              onDragOver={(e) => {
                e.preventDefault();
                if (isAdmin) setDragging(true);
              }}
              onDragLeave={() => setDragging(false)}
              onDrop={onDrop}
              className={cn(
                "flex flex-col items-center gap-1.5 rounded-xl border-[1.5px] border-dashed border-[#c3c8d4] bg-[#fafbfc] p-[22px] text-center",
                isAdmin ? "cursor-pointer" : "cursor-not-allowed opacity-60",
                dragging && "border-primary bg-accent",
              )}
            >
              <Upload className="size-5 text-subtle-foreground" aria-hidden />
              <span className="text-sm font-medium">Drop a file, or browse</span>
              <span className="text-xs text-subtle-foreground">
                CSV, TSV, Excel, Parquet, JSON or SQLite up to 200 MB · every sheet becomes a table
              </span>
              <input
                ref={fileInput}
                id="upload"
                type="file"
                accept={ACCEPTED_FILES}
                disabled={!isAdmin}
                className="sr-only"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  e.target.value = "";
                  if (file) void startUpload(file);
                }}
              />
            </label>
          </section>

          {upload.phase !== "idle" && <UploadProgress state={upload} projectId={projectId} />}
        </div>
      </div>

      <DatabaseDialog
        projectId={projectId}
        kind={dialog}
        onClose={() => setDialog(null)}
        onCreated={(detail) => {
          setDialog(null);
          router.push(`/projects/${projectId}/sources/${detail.source.id}`);
        }}
      />
    </>
  );
}

function UploadProgress({ state, projectId }: { state: UploadState; projectId: string }) {
  const name =
    state.phase === "done" ? state.detail.source.name : "name" in state ? state.name : "";
  const steps: { label: string; status: "done" | "active" | "todo" | "failed" }[] = (() => {
    switch (state.phase) {
      case "uploading":
        return [
          { label: `Uploading ${Math.round(state.fraction * 100)}%`, status: "active" },
          { label: "Loading into the project database", status: "todo" },
          { label: "Profiling every column", status: "todo" },
        ];
      case "processing":
        return [
          { label: "Uploaded", status: "done" },
          { label: "Loading and profiling…", status: "active" },
          { label: "Ready to explore", status: "todo" },
        ];
      case "done": {
        const tables = state.detail.tables;
        const rows = tables.reduce((n, t) => n + (t.row_count ?? 0), 0);
        return [
          { label: "Uploaded", status: "done" },
          {
            label: `Loaded ${pluralize(tables.length, "table")}, ${formatCount(rows)} rows`,
            status: "done",
          },
          {
            label: `Profiled ${pluralize(
              tables.reduce((n, t) => n + t.columns.length, 0),
              "column",
            )}`,
            status: "done",
          },
        ];
      }
      case "failed":
        return [{ label: state.message, status: "failed" }];
      default:
        return [];
    }
  })();
  return (
    <section
      aria-live="polite"
      className="flex flex-col gap-3.5 rounded-xl border border-border bg-card p-5"
    >
      <div className="flex flex-col">
        <h2 className="text-base font-semibold">Adding {name}</h2>
        <span className="text-xs text-subtle-foreground">
          {state.phase === "done" ? "Done" : state.phase === "failed" ? "Failed" : "Working…"}
        </span>
      </div>
      <ol className="flex flex-col gap-2.5 text-sm">
        {steps.map((step) => (
          <li key={step.label} className="flex items-center gap-2.5">
            <span
              className={cn(
                "flex size-[18px] flex-none items-center justify-center rounded-full text-[11px] text-white",
                step.status === "done" && "bg-[#0F766E]",
                step.status === "failed" && "bg-destructive",
                step.status === "active" && "border-2 border-primary",
                step.status === "todo" && "border-2 border-input",
              )}
            >
              {step.status === "done" ? "✓" : step.status === "failed" ? "!" : ""}
            </span>
            <span className={cn(step.status === "todo" && "text-subtle-foreground")}>
              {step.label}
            </span>
          </li>
        ))}
      </ol>
      {state.phase === "done" && (
        <Link
          href={`/projects/${projectId}/sources/${state.detail.source.id}`}
          className="self-start text-sm font-medium no-underline"
        >
          Explore {state.detail.source.name} →
        </Link>
      )}
    </section>
  );
}
