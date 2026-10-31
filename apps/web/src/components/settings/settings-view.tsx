"use client";

import { useRouter } from "next/navigation";

import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { capitalize } from "@/lib/format";
import { useLogout, useMe } from "@/lib/queries";
import { useUiStore } from "@/lib/ui-store";

export function SettingsView() {
  const me = useMe();
  const logout = useLogout();
  const router = useRouter();
  const perfOpen = useUiStore((s) => s.perfOverlayOpen);
  const togglePerf = useUiStore((s) => s.togglePerfOverlay);

  return (
    <div className="grid max-w-[720px] gap-4">
      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card px-5 py-[18px]">
        <h2 className="text-[15px] font-semibold">Account</h2>
        {me.data ? (
          <dl className="grid grid-cols-[160px_1fr] gap-y-2 text-sm">
            <dt className="text-subtle-foreground">Name</dt>
            <dd>{me.data.user.display_name}</dd>
            <dt className="text-subtle-foreground">Email</dt>
            <dd>{me.data.user.email}</dd>
            <dt className="text-subtle-foreground">Workspace</dt>
            <dd>
              {me.data.workspace.name} · {capitalize(me.data.workspace_role)}
            </dd>
            <dt className="text-subtle-foreground">Session ends</dt>
            <dd>{new Date(me.data.session_expires_at).toLocaleString()}</dd>
          </dl>
        ) : (
          <Skeleton className="h-24 w-full" />
        )}
        <Button
          variant="outline"
          className="self-start"
          disabled={logout.isPending}
          onClick={() => logout.mutate(undefined, { onSettled: () => router.replace("/login") })}
        >
          Sign out
        </Button>
      </section>
      <section className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card px-5 py-[18px]">
        <div className="flex flex-col gap-1">
          <h2 className="text-[15px] font-semibold">Performance overlay</h2>
          <p className="text-sm text-muted-foreground">
            Page vitals, main-thread health, WebSocket round trip and gateway timings. Shortcut:
            Ctrl+Shift+P.
          </p>
        </div>
        <Button variant="outline" onClick={togglePerf}>
          {perfOpen ? "Hide" : "Show"}
        </Button>
      </section>
    </div>
  );
}
