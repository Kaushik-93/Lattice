"use client";

import type { Sheet, SheetSummary } from "@lattice/shared-types";
import { useQueryClient } from "@tanstack/react-query";
import { Link2, MoreHorizontal, Pencil, Plus, Redo2, Trash2, Undo2 } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { isSignedOutError } from "@/components/shell/auth-gate";
import { ErrorState } from "@/components/shell/page-skeleton";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { useProjectModel } from "@/lib/knowledge/api";
import { dataClient, type QueryResult } from "@/lib/data/client";
import { ApiError, api } from "@/lib/gateway";
import { useProject, useProjectTables } from "@/lib/queries";
import {
  prefetchSheet,
  saveSheet,
  sheetKeys,
  sheetSaved,
  useCreateSheet,
  useDeleteSheet,
  useSheet,
  useSheets,
} from "@/lib/sheets/api";
import { buildContext } from "@/lib/sheets/context";
import { VisualResults, splitQueryId } from "@/lib/sheets/results";
import { useSheetRunner, type Settled } from "@/lib/sheets/runner";
import { createSheetStore, type SheetStore } from "@/lib/sheets/store";
import { cn } from "@/lib/utils";

import { SheetEnvProvider, useSheetEnv, useSheetState, type SheetEnv } from "./sheet-env";
import { SheetFilters, usePresetDefaults } from "./sheet-filters";
import { SheetGrid } from "./sheet-grid";
import { EditorPanel, addVisual } from "./visual-editor";

const SHEET_PATH = /\/sheets\/([^/?#]+)/;
/** Autosave this long after the last edit. */
const SAVE_DELAY_MS = 800;

/**
 * Results per sheet, kept across tab switches: a sheet you come back to (or that was
 * prefetched as a neighbour) shows its data at once while the fresh batch runs.
 */
const resultsBySheet = new Map<string, VisualResults>();
function resultsFor(sheetId: string): VisualResults {
  let r = resultsBySheet.get(sheetId);
  if (!r) {
    r = new VisualResults();
    resultsBySheet.set(sheetId, r);
  }
  return r;
}

export function sheetHref(projectId: string, sheetId: string): string {
  return `/projects/${projectId}/sheets/${sheetId}`;
}

const SWITCH_MARK = "lattice:sheet-switch";

/** Switches tabs without a server round trip (spec §3: sheet switch < 100 ms). The mark lets
 * the new sheet report click → settled. */
function goTo(projectId: string, sheetId: string): void {
  performance.clearMarks(SWITCH_MARK);
  performance.mark(SWITCH_MARK);
  window.history.pushState(null, "", sheetHref(projectId, sheetId));
}

/** Design screen 06: a project's sheets as tabs, the open one below. */
export function SheetsView({ projectId }: { projectId: string }) {
  const pathname = usePathname();
  const sheets = useSheets(projectId);
  const project = useProject(projectId);
  const sheetId = SHEET_PATH.exec(pathname)?.[1] ?? sheets.data?.[0]?.id;
  const role = project.data?.project.my_role;
  const canEdit = role === "editor" || role === "admin";

  // /projects/:id/sheets → the first sheet.
  useEffect(() => {
    if (!SHEET_PATH.test(pathname) && sheets.data?.[0]) {
      window.history.replaceState(null, "", sheetHref(projectId, sheets.data[0].id));
    }
  }, [pathname, sheets.data, projectId]);

  if (sheets.isError && !isSignedOutError(sheets.error)) {
    return <ErrorState title="Couldn't load sheets" message={sheets.error.message} />;
  }
  if (sheets.data && sheets.data.length === 0) {
    return (
      <EmptySheets
        projectId={projectId}
        projectName={project.data?.project.name}
        canEdit={canEdit}
      />
    );
  }
  if (!sheetId || !project.data) return <SheetSkeleton />;
  return (
    <OpenSheet
      key={sheetId}
      projectId={projectId}
      sheetId={sheetId}
      canEdit={canEdit}
      tabs={sheets.data ?? []}
      projectName={project.data.project.name}
    />
  );
}

function OpenSheet(props: {
  projectId: string;
  sheetId: string;
  canEdit: boolean;
  tabs: SheetSummary[];
  projectName: string;
}) {
  const sheet = useSheet(props.projectId, props.sheetId);
  if (sheet.isError) {
    const gone = sheet.error instanceof ApiError && sheet.error.status === 404;
    return (
      <ErrorState
        title={gone ? "This sheet doesn't exist" : "Couldn't load the sheet"}
        message={gone ? "It may have been deleted." : sheet.error.message}
      />
    );
  }
  if (!sheet.data) return <SheetSkeleton />;
  return <SheetEditor {...props} sheet={sheet.data} />;
}

function SheetEditor({
  projectId,
  sheet,
  canEdit,
  tabs,
  projectName,
}: {
  projectId: string;
  sheet: Sheet;
  canEdit: boolean;
  tabs: SheetSummary[];
  projectName: string;
}) {
  const client = useQueryClient();
  const [store] = useState(() => createSheetStore(projectId, sheet, canEdit));
  const results = resultsFor(sheet.id);
  const rootRef = useRef<HTMLDivElement>(null);
  const target = useMemo(() => ({ kind: "member" as const, projectId }), [projectId]);
  const prefetched = useRef(false);

  const onSettled = useCallback(
    (settled: Settled) => {
      const root = rootRef.current;
      if (root) {
        root.dataset.settled = String(Number(root.dataset.settled ?? 0) + 1);
        root.dataset.settleMs = settled.ms.toFixed(1);
        const mark = performance.getEntriesByName(SWITCH_MARK)[0];
        if (mark) {
          root.dataset.switchMs = (performance.now() - mark.startTime).toFixed(1);
          performance.clearMarks(SWITCH_MARK);
        }
      }
      if (!prefetched.current) {
        prefetched.current = true;
        void prefetchNeighbours(client, projectId, sheet.id, tabs);
      }
    },
    [client, projectId, sheet.id, tabs],
  );
  useSheetRunner(target, store, results, onSettled);
  useAutosave(store, projectId, sheet.id, client);
  useUndoKeys(store, canEdit);

  const fetchPage = useCallback(
    async (visualId: string, offset: number): Promise<QueryResult | null> => {
      const s = store.getState();
      const visual = s.doc.visuals.find((v) => v.id === visualId);
      if (!visual) return null;
      const context = buildContext(s.doc.filters, s.filterValues, s.selections);
      const outcome = await dataClient(`page:${visualId}`).runSheet(projectId, context, [
        { id: visualId, spec: visual.spec, drill: s.drill[visualId] ?? [], offset },
      ]);
      return outcome.results.get(visualId) ?? null;
    },
    [store, projectId],
  );
  const model = useProjectModel(projectId);
  const fiscalStart = model.data?.fiscal_year_start_month ?? 1;
  usePresetDefaults(store, model.data?.fiscal_year_start_month);
  const env = useMemo<SheetEnv>(
    () => ({ store, results, target, canEdit, canExport: true, fetchPage, fiscalStart }),
    [store, results, target, canEdit, fetchPage, fiscalStart],
  );

  return (
    <SheetEnvProvider value={env}>
      <SheetBody
        rootRef={rootRef}
        projectId={projectId}
        projectName={projectName}
        sheetId={sheet.id}
        tabs={tabs}
      />
    </SheetEnvProvider>
  );
}

/** Loads the sheets on either side and runs their visuals in the background (spec §3 rule 5). */
async function prefetchNeighbours(
  client: ReturnType<typeof useQueryClient>,
  projectId: string,
  sheetId: string,
  tabs: SheetSummary[],
): Promise<void> {
  const i = tabs.findIndex((t) => t.id === sheetId);
  const neighbours = [tabs[i + 1], tabs[i - 1]].filter((t): t is SheetSummary => t !== undefined);
  for (const tab of neighbours) {
    try {
      const sheet = await client.fetchQuery({
        queryKey: sheetKeys.sheet(projectId, tab.id),
        queryFn: ({ signal }) =>
          api<Sheet>(`/api/projects/${projectId}/sheets/${tab.id}`, { signal }),
        staleTime: Infinity,
      });
      if (sheet.visuals.length === 0) continue;
      const results = resultsFor(tab.id);
      const context = buildContext(sheet.filters, {}, []);
      await dataClient("prefetch").runSheet(
        projectId,
        context,
        sheet.visuals.map((v) => ({ id: v.id, spec: v.spec, drill: [] })),
        (r) => {
          const { id, part } = splitQueryId(r.header.query_id);
          results.put(id, part, r);
        },
      );
    } catch {
      // Prefetching is opportunistic.
    }
  }
}

function useAutosave(
  store: SheetStore,
  projectId: string,
  sheetId: string,
  client: ReturnType<typeof useQueryClient>,
) {
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const save = async () => {
      const s = store.getState();
      if (s.saveState !== "dirty") return;
      s.saving();
      const doc = s.doc;
      try {
        const { version } = await saveSheet(projectId, sheetId, { version: s.version, ...doc });
        store.getState().saved(version);
        sheetSaved(client, projectId, sheetId, {
          name: doc.name,
          visual_count: doc.visuals.length,
        });
        client.setQueryData<Sheet>(sheetKeys.sheet(projectId, sheetId), (old) =>
          old ? { ...old, ...doc, version } : old,
        );
        if (store.getState().saveState === "dirty")
          timer = setTimeout(() => void save(), SAVE_DELAY_MS);
      } catch (error) {
        const conflict = error instanceof ApiError && error.status === 409;
        store
          .getState()
          .failed(
            conflict ? "conflict" : "error",
            error instanceof Error ? error.message : "Couldn't save.",
          );
      }
    };
    const unsubscribe = store.subscribe((s, prev) => {
      if (s.doc !== prev.doc && s.saveState === "dirty") {
        clearTimeout(timer);
        timer = setTimeout(() => void save(), SAVE_DELAY_MS);
      }
    });
    const warn = (e: BeforeUnloadEvent) => {
      const state = store.getState().saveState;
      if (state === "dirty" || state === "saving") e.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => {
      unsubscribe();
      window.removeEventListener("beforeunload", warn);
      clearTimeout(timer);
      void save();
    };
  }, [store, projectId, sheetId, client]);
}

function useUndoKeys(store: SheetStore, canEdit: boolean) {
  useEffect(() => {
    if (!canEdit) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "z") return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      e.preventDefault();
      if (e.shiftKey) store.getState().redo();
      else store.getState().undo();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [store, canEdit]);
}

function SheetBody({
  rootRef,
  projectId,
  projectName,
  sheetId,
  tabs,
}: {
  rootRef: React.RefObject<HTMLDivElement | null>;
  projectId: string;
  projectName: string;
  sheetId: string;
  tabs: SheetSummary[];
}) {
  const { store, canEdit } = useSheetEnv();
  const name = useSheetState((s) => s.doc.name);
  const editing = useSheetState((s) => s.editing);
  const empty = useSheetState((s) => s.doc.visuals.length === 0);
  const { tables } = useProjectTables(canEdit ? projectId : "");
  return (
    <div
      ref={rootRef}
      data-testid="sheet"
      data-sheet={sheetId}
      className={cn("flex flex-col gap-5", editing && "lg:pr-[340px]")}
    >
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1.5">
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
              {projectName}
            </Link>{" "}
            / Sheets
          </span>
          <h1 className="truncate text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
            {name || "Untitled sheet"}
          </h1>
        </div>
        <div className="flex flex-wrap items-center gap-2.5">
          <SheetFilters />
          {canEdit && <EditControls />}
          {canEdit && (
            <Link
              href={`/projects/${projectId}/sharing?sheet=${sheetId}`}
              className="inline-flex h-10 items-center gap-2 rounded-[10px] bg-primary px-4 text-sm font-semibold text-primary-foreground no-underline hover:bg-primary-hover hover:text-primary-foreground"
            >
              <Link2 className="size-4" aria-hidden /> Share sheet
            </Link>
          )}
        </div>
      </header>
      <SheetTabs projectId={projectId} activeId={sheetId} tabs={tabs} canEdit={canEdit} />
      <SaveBanner />
      {empty ? (
        <div className="flex flex-col items-start gap-3 rounded-xl border border-dashed border-input bg-card p-8">
          <h2 className="text-lg font-semibold">This sheet is empty</h2>
          <p className="max-w-prose text-sm text-muted-foreground">
            {canEdit
              ? "Add a visual to start: pick a table, a chart type and the fields to show."
              : "An editor hasn't added any visuals yet."}
          </p>
          {canEdit && (
            <Button onClick={() => addVisual(store, tables)} disabled={tables.length === 0}>
              <Plus className="size-4" /> Add visual
            </Button>
          )}
          {canEdit && tables.length === 0 && (
            <p className="text-sm text-muted-foreground">
              Connect a source first: <Link href={`/projects/${projectId}/sources`}>Sources</Link>.
            </p>
          )}
        </div>
      ) : (
        <SheetGrid />
      )}
      {editing && canEdit && <EditorPanel />}
    </div>
  );
}

function EditControls() {
  const { store, target } = useSheetEnv();
  const editing = useSheetState((s) => s.editing);
  const canUndo = useSheetState((s) => s.past.length > 0);
  const canRedo = useSheetState((s) => s.future.length > 0);
  const undoLabel = useSheetState((s) => s.past.at(-1)?.label);
  const redoLabel = useSheetState((s) => s.future[0]?.label);
  const saveState = useSheetState((s) => s.saveState);
  const projectId = target.kind === "member" ? target.projectId : "";
  const { tables } = useProjectTables(projectId);
  if (!editing) {
    return (
      <Button variant="outline" className="h-10" onClick={() => store.getState().setEditing(true)}>
        <Pencil className="size-4" /> Edit
      </Button>
    );
  }
  return (
    <div className="flex items-center gap-1.5">
      <span
        className="mr-1 text-xs text-muted-foreground"
        aria-live="polite"
        data-testid="save-state"
      >
        {saveState === "saving"
          ? "Saving…"
          : saveState === "dirty"
            ? "Unsaved"
            : saveState === "saved"
              ? "Saved"
              : ""}
      </span>
      <Button
        variant="outline"
        size="icon"
        className="size-10"
        aria-label={undoLabel ? `Undo ${undoLabel.toLowerCase()}` : "Undo"}
        title={undoLabel ? `Undo ${undoLabel.toLowerCase()} (⌘Z)` : "Nothing to undo"}
        disabled={!canUndo}
        onClick={() => store.getState().undo()}
      >
        <Undo2 className="size-4" />
      </Button>
      <Button
        variant="outline"
        size="icon"
        className="size-10"
        aria-label={redoLabel ? `Redo ${redoLabel.toLowerCase()}` : "Redo"}
        title={redoLabel ? `Redo ${redoLabel.toLowerCase()} (⇧⌘Z)` : "Nothing to redo"}
        disabled={!canRedo}
        onClick={() => store.getState().redo()}
      >
        <Redo2 className="size-4" />
      </Button>
      <Button
        variant="outline"
        className="h-10"
        onClick={() => addVisual(store, tables)}
        disabled={tables.length === 0}
      >
        <Plus className="size-4" /> Add visual
      </Button>
      <Button className="h-10" onClick={() => store.getState().setEditing(false)}>
        Done
      </Button>
    </div>
  );
}

function SaveBanner() {
  const saveState = useSheetState((s) => s.saveState);
  const message = useSheetState((s) => s.saveError);
  if (saveState !== "conflict" && saveState !== "error") return null;
  return (
    <div
      role="alert"
      className="flex flex-wrap items-center gap-3 rounded-[10px] border border-destructive/30 bg-destructive-surface px-4 py-3 text-sm text-destructive"
    >
      <span className="flex-1">{message}</span>
      <Button variant="outline" size="sm" onClick={() => window.location.reload()}>
        Reload
      </Button>
    </div>
  );
}

function SheetTabs({
  projectId,
  activeId,
  tabs,
  canEdit,
}: {
  projectId: string;
  activeId: string;
  tabs: SheetSummary[];
  canEdit: boolean;
}) {
  const client = useQueryClient();
  const create = useCreateSheet(projectId);
  const remove = useDeleteSheet(projectId);
  const newSheet = () =>
    create.mutate(
      { name: `Sheet ${tabs.length + 1}` },
      { onSuccess: (sheet) => goTo(projectId, sheet.id) },
    );
  const deleteActive = () => {
    const next = tabs.find((t) => t.id !== activeId);
    remove.mutate(activeId, {
      onSuccess: () => {
        if (next) goTo(projectId, next.id);
        else window.history.pushState(null, "", `/projects/${projectId}/sheets`);
      },
    });
  };
  return (
    <div className="flex items-end gap-1 border-b border-border">
      <div
        role="tablist"
        aria-label="Sheets"
        className="flex min-w-0 flex-1 items-end gap-1 overflow-x-auto"
      >
        {tabs.map((tab) => {
          const active = tab.id === activeId;
          return (
            <a
              key={tab.id}
              role="tab"
              aria-selected={active}
              href={sheetHref(projectId, tab.id)}
              onClick={(e) => {
                if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
                e.preventDefault();
                if (!active) goTo(projectId, tab.id);
              }}
              onMouseEnter={() => prefetchSheet(client, projectId, tab.id)}
              onFocus={() => prefetchSheet(client, projectId, tab.id)}
              className={cn(
                "-mb-px flex h-11 flex-none items-center gap-2 border-b-2 px-3.5 text-sm font-medium whitespace-nowrap no-underline",
                active
                  ? "border-primary text-accent-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              {tab.name}
              {tab.link_audience !== "off" && (
                <span className="rounded-full bg-success-surface px-2 py-0.5 text-[11px] font-medium text-success">
                  View link
                </span>
              )}
            </a>
          );
        })}
        {canEdit && (
          <button
            type="button"
            onClick={newSheet}
            disabled={create.isPending}
            className="mb-1.5 ml-1 h-8 flex-none rounded-lg border border-dashed border-input px-3 text-sm text-muted-foreground hover:border-primary hover:text-accent-foreground"
          >
            + New sheet
          </button>
        )}
      </div>
      {canEdit && (
        <DropdownMenu>
          <DropdownMenuTrigger
            aria-label="Sheet actions"
            className="mb-1.5 grid size-8 place-items-center rounded-md text-subtle-foreground hover:bg-secondary"
          >
            <MoreHorizontal className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem variant="destructive" onSelect={deleteActive}>
              <Trash2 /> Delete this sheet
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}

function EmptySheets({
  projectId,
  projectName,
  canEdit,
}: {
  projectId: string;
  projectName?: string;
  canEdit: boolean;
}) {
  const create = useCreateSheet(projectId);
  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">Sheets</h1>
      <div className="flex flex-col items-start gap-3 rounded-xl border border-dashed border-input bg-card p-8">
        <h2 className="text-lg font-semibold">{projectName ?? "This project"} has no sheets yet</h2>
        <p className="text-sm text-muted-foreground">
          A sheet is a page of visuals over this project&apos;s sources, with filters that apply to
          all of them.
        </p>
        {canEdit && (
          <Button
            onClick={() =>
              create.mutate({ name: "Sheet 1" }, { onSuccess: (s) => goTo(projectId, s.id) })
            }
            disabled={create.isPending}
          >
            <Plus className="size-4" /> New sheet
          </Button>
        )}
      </div>
    </div>
  );
}

function SheetSkeleton() {
  return (
    <div className="flex flex-col gap-5" aria-busy="true" aria-label="Loading sheet">
      <Skeleton className="h-9 w-64" />
      <Skeleton className="h-11 w-full" />
      <div className="grid grid-cols-4 gap-4">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-28" />
        ))}
      </div>
      <Skeleton className="h-64 w-full" />
    </div>
  );
}
