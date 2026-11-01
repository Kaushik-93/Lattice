"use client";

import type { SharedSheetView } from "@lattice/shared-types";
import { Eye } from "lucide-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useCallback, useMemo, useState } from "react";

import { LogoMark } from "@/components/brand/logo";
import { Skeleton } from "@/components/ui/skeleton";
import { dataClient, type QueryResult } from "@/lib/data/client";
import { timeAgo } from "@/lib/format";
import { ApiError } from "@/lib/gateway";
import { useMe } from "@/lib/queries";
import { useSharedSheet } from "@/lib/sheets/api";
import { buildContext } from "@/lib/sheets/context";
import { VisualResults } from "@/lib/sheets/results";
import { useSheetRunner } from "@/lib/sheets/runner";
import { createSheetStore } from "@/lib/sheets/store";

import { SheetEnvProvider, type SheetEnv } from "./sheet-env";
import { SheetFilters } from "./sheet-filters";
import { SheetGrid } from "./sheet-grid";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function longDate(iso: string): string {
  const d = new Date(iso);
  return `${MONTHS[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`;
}

/** Design screen 08: a sheet opened through its link — view access only, no project shell. */
export function SharedSheet() {
  const { token } = useParams<{ token: string }>();
  const shared = useSharedSheet(token);
  const me = useMe();
  const signedIn = me.isSuccess;

  if (shared.isError) {
    const status = shared.error instanceof ApiError ? shared.error.status : 0;
    const [title, message] =
      status === 401
        ? ["Sign in to open this sheet", "Its owner shared it with signed-in people only."]
        : status === 403
          ? ["This sheet isn't shared with you", "Ask its owner to invite you."]
          : status === 404
            ? ["This link doesn't work", "It may have expired or been turned off."]
            : ["Couldn't open this sheet", shared.error.message];
    return (
      <Frame signedIn={signedIn} token={token}>
        <div
          role="alert"
          className="mx-auto mt-16 flex max-w-md flex-col items-start gap-3 rounded-xl border border-border bg-card p-8"
        >
          <h1 className="text-xl font-semibold">{title}</h1>
          <p className="text-sm text-muted-foreground">{message}</p>
          {status === 401 && (
            <Link
              href={`/login?next=${encodeURIComponent(`/s/${token}`)}`}
              className="text-sm font-semibold"
            >
              Sign in →
            </Link>
          )}
        </div>
      </Frame>
    );
  }
  if (!shared.data) {
    return (
      <Frame signedIn={signedIn} token={token}>
        <div className="flex flex-col gap-5" aria-busy="true" aria-label="Loading sheet">
          <Skeleton className="h-10 w-80" />
          <div className="grid grid-cols-3 gap-4">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="h-28" />
            ))}
          </div>
          <Skeleton className="h-72" />
        </div>
      </Frame>
    );
  }
  return (
    <Frame signedIn={signedIn} token={token} view={shared.data}>
      <SharedBody key={shared.data.sheet.id} token={token} view={shared.data} />
    </Frame>
  );
}

function Frame({
  children,
  signedIn,
  token,
  view,
}: {
  children: React.ReactNode;
  signedIn: boolean;
  token: string;
  view?: SharedSheetView;
}) {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <header className="border-b border-border bg-card">
        <div className="mx-auto flex max-w-[1180px] items-center gap-3 px-6 py-3.5">
          <LogoMark size={26} />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[15px] font-semibold">
              {view?.sheet.name ?? "Shared sheet"}
            </span>
            {view && (
              <span className="truncate text-xs text-subtle-foreground">
                {view.project_name}
                {view.shared_by ? ` · shared by ${view.shared_by}` : ""}
              </span>
            )}
          </div>
          <span className="flex items-center gap-1.5 rounded-full bg-secondary px-2.5 py-1 text-xs font-medium text-secondary-foreground">
            <Eye className="size-3.5" aria-hidden /> View access
          </span>
          {!signedIn && (
            <Link
              href={`/login?next=${encodeURIComponent(`/s/${token}`)}`}
              className="h-10 rounded-[10px] border border-input px-4 text-sm leading-10 font-semibold text-foreground no-underline hover:bg-secondary hover:text-foreground"
            >
              Sign in
            </Link>
          )}
        </div>
      </header>
      <main className="mx-auto flex w-full max-w-[1180px] flex-1 flex-col gap-5 px-6 py-7">
        {children}
      </main>
      <footer className="border-t border-border">
        <div className="mx-auto flex max-w-[1180px] flex-wrap justify-between gap-2 px-6 py-3.5 text-xs text-subtle-foreground">
          <span>
            {view?.data_refreshed_at ? `Data refreshed ${timeAgo(view.data_refreshed_at)}` : ""}
            {view?.expires_at ? ` · Link expires ${longDate(view.expires_at)}` : ""}
          </span>
          <span>Made with Lattice</span>
        </div>
      </footer>
    </div>
  );
}

function SharedBody({ token, view }: { token: string; view: SharedSheetView }) {
  const [store] = useState(() => createSheetStore(view.sheet.project_id, view.sheet, false));
  const [results] = useState(() => new VisualResults());
  const target = useMemo(() => ({ kind: "shared" as const, token }), [token]);
  useSheetRunner(target, store, results);

  const fetchPage = useCallback(
    async (visualId: string, offset: number): Promise<QueryResult | null> => {
      const s = store.getState();
      const context = buildContext(s.doc.filters, s.filterValues, s.selections);
      const outcome = await dataClient(`page:${visualId}`).runShared(token, context, [
        { id: visualId, drill: s.drill[visualId] ?? [], offset },
      ]);
      return outcome.results.get(visualId) ?? null;
    },
    [store, token],
  );
  const env = useMemo<SheetEnv>(
    () => ({ store, results, target, canEdit: false, canExport: view.allow_export, fetchPage }),
    [store, results, target, view.allow_export, fetchPage],
  );

  return (
    <SheetEnvProvider value={env}>
      <div data-testid="shared-sheet" className="flex flex-wrap items-center justify-between gap-3">
        <SheetFilters segmented />
      </div>
      <SheetGrid />
      <p className="text-xs text-muted-foreground">
        Viewers can explore and filter, but can&apos;t edit, save or see other sheets.
      </p>
    </SheetEnvProvider>
  );
}
