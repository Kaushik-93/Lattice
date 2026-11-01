"use client";

import type { ProjectMember } from "@lattice/shared-types";
import { Share2 } from "lucide-react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import type { ReactNode } from "react";

import { SparkleIcon } from "@/components/auth/auth-aside";
import { ErrorState } from "@/components/shell/page-skeleton";
import { isSignedOutError } from "@/components/shell/auth-gate";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { SourceBadge, statusDot, statusLabel } from "@/components/sources/source-meta";
import { capitalize, updatedLabel } from "@/lib/format";
import { useProject, useSources } from "@/lib/queries";
import { useCreateSheet, useSheets } from "@/lib/sheets/api";

import { ProjectTile } from "./project-tile";

export function ProjectOverview() {
  const { projectId } = useParams<{ projectId: string }>();
  const query = useProject(projectId);

  if (query.isPending || isSignedOutError(query.error)) return <ProjectOverviewSkeleton />;
  if (query.error) {
    const notFound = "status" in query.error && query.error.status === 404;
    return (
      <ErrorState
        title={notFound ? "Project not found" : "Couldn't load this project"}
        message={
          notFound ? "It doesn't exist, or you don't have access to it." : query.error.message
        }
        action={
          <Link href="/projects" className="text-sm font-medium no-underline">
            ← All projects
          </Link>
        }
      />
    );
  }

  const { project, members } = query.data;
  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex items-center gap-4">
          <ProjectTile name={project.name} color={project.color} size={52} />
          <div className="flex flex-col gap-1">
            <span className="text-[13px] text-subtle-foreground">
              <Link
                href="/projects"
                className="text-subtle-foreground no-underline hover:text-foreground"
              >
                Projects
              </Link>{" "}
              / {project.name}
            </span>
            <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
              {project.name}
            </h1>
            {project.description && (
              <p className="text-sm text-muted-foreground">{project.description}</p>
            )}
          </div>
        </div>
        <div className="flex flex-wrap gap-2.5">
          <Button variant="outline" asChild>
            <Link href={`/projects/${project.id}/sharing`} className="no-underline">
              <Share2 /> Share project
            </Link>
          </Button>
          {(project.my_role === "editor" || project.my_role === "admin") && (
            <NewSheetButton projectId={project.id} count={project.sheet_count} />
          )}
        </div>
      </header>

      <section id="sheets" aria-labelledby="sheets-h" className="flex scroll-mt-8 flex-col gap-3">
        <div className="flex items-baseline justify-between">
          <h2 id="sheets-h" className="text-[17px] font-semibold">
            Sheets{" "}
            <span className="text-sm font-normal text-subtle-foreground">
              {project.sheet_count}
            </span>
          </h2>
          <span className="text-[13px] text-subtle-foreground">
            Each sheet can be shared on its own
          </span>
        </div>
        <SheetCards projectId={project.id} />
      </section>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,320px),1fr))] items-start gap-4">
        <SourcesPanel projectId={project.id} />

        <Panel id="members" title="Members" count={members.length}>
          {members.map((member) => (
            <MemberRow key={member.user_id} member={member} />
          ))}
        </Panel>

        <Panel
          title={
            <span className="flex items-center gap-2">
              <SparkleIcon size={18} color="#4338CA" /> Agent digest · this week
            </span>
          }
        >
          <Empty>
            The digest lists what the agents learned, proposed and flagged in this project. It
            starts once sources are connected.
          </Empty>
        </Panel>
      </div>
    </>
  );
}

function SourcesPanel({ projectId }: { projectId: string }) {
  const sources = useSources(projectId);
  return (
    <Panel
      id="sources"
      title="Sources in this project"
      action={
        <Link
          href={`/projects/${projectId}/sources`}
          className="text-[13px] font-medium no-underline"
        >
          Manage
        </Link>
      }
    >
      {sources.isPending ? (
        <Skeleton className="h-24 w-full" />
      ) : sources.data && sources.data.length > 0 ? (
        sources.data.map((s) => (
          <Link
            key={s.id}
            href={`/projects/${projectId}/sources/${s.id}`}
            className="flex items-center gap-2.5 text-sm text-foreground no-underline hover:text-primary"
          >
            <SourceBadge source={s} size={30} />
            <span className="min-w-0 flex-1 truncate">{s.name}</span>
            <span
              aria-hidden="true"
              className="size-2 rounded-full"
              style={{ background: statusDot(s.status) }}
            />
            <span className="w-[92px] text-right text-xs text-subtle-foreground">
              {statusLabel(s.status)}
            </span>
          </Link>
        ))
      ) : (
        <Empty>
          No sources connected yet. Add files, PostgreSQL, MySQL or SQLite from the Sources page;
          every new source is profiled so you can see what&apos;s inside.
        </Empty>
      )}
    </Panel>
  );
}

function MemberRow({ member }: { member: ProjectMember }) {
  return (
    <div className="flex items-center gap-2.5">
      <span className="flex size-8 flex-none items-center justify-center rounded-full bg-avatar text-[11px] font-semibold text-avatar-foreground">
        {member.initials}
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm font-medium">{member.display_name}</span>
        <span className="truncate text-xs text-subtle-foreground">{member.email}</span>
      </div>
      <span className="text-xs text-secondary-foreground">
        {member.is_owner ? "Owner" : capitalize(member.role)}
      </span>
    </div>
  );
}

function Panel({
  id,
  title,
  count,
  action,
  children,
}: {
  id?: string;
  title: ReactNode;
  count?: number;
  action?: ReactNode;
  children: ReactNode;
}) {
  const headingId = id ? `${id}-h` : undefined;
  return (
    <section
      id={id}
      aria-labelledby={headingId}
      className="flex scroll-mt-8 flex-col gap-3 rounded-xl border border-border bg-card px-5 py-[18px]"
    >
      <div className="flex items-baseline justify-between gap-2">
        <h2 id={headingId} className="text-[15px] font-semibold">
          {title}{" "}
          {count !== undefined && (
            <span className="font-normal text-subtle-foreground">{count}</span>
          )}
        </h2>
        {action}
      </div>
      {children}
    </section>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return (
    <p className="rounded-[10px] border border-dashed border-input bg-[#fafbfc] px-4 py-5 text-sm leading-relaxed text-muted-foreground">
      {children}
    </p>
  );
}

export function ProjectOverviewSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy="true" aria-label="Loading project">
      <div className="flex items-center gap-4">
        <Skeleton className="size-[52px] rounded-[14px]" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-3 w-40" />
          <Skeleton className="h-7 w-64" />
          <Skeleton className="h-4 w-96 max-w-full" />
        </div>
      </div>
      <Skeleton className="h-24 w-full rounded-xl" />
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,320px),1fr))] gap-4">
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} className="h-44 rounded-xl" />
        ))}
      </div>
    </div>
  );
}

const THUMB_COLORS = ["#4338ca", "#0f766e", "#ea7a2e", "#98a2b3"];

/** Seven bar heights (40–99 %) derived from the id, so a sheet's thumbnail never changes. */
function thumbBars(id: string): number[] {
  let seed = [...id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  return Array.from({ length: 7 }, () => {
    seed = (seed * 1103515245 + 12345) >>> 0;
    return 40 + (seed % 60);
  });
}

/** A decorative bar strip (design screen 03). */
function SheetThumb({ id, index }: { id: string; index: number }) {
  const bars = thumbBars(id);
  return (
    <div
      aria-hidden
      className="flex h-[108px] items-end gap-1.5 rounded-t-xl bg-[#fafbfc] px-3.5 pt-5"
    >
      {bars.map((h, i) => (
        <span
          key={i}
          className="flex-1 rounded-t-sm"
          style={{ height: `${h}%`, background: THUMB_COLORS[index % 4] }}
        />
      ))}
    </div>
  );
}

function SheetCards({ projectId }: { projectId: string }) {
  const sheets = useSheets(projectId);
  if (sheets.isPending) {
    return (
      <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} className="h-[200px] rounded-xl" />
        ))}
      </div>
    );
  }
  if (!sheets.data?.length) {
    return (
      <Empty>
        No sheets yet. A sheet is a grid of charts, tables and filters over this project&apos;s
        sources.
      </Empty>
    );
  }
  return (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
      {sheets.data.map((sheet, i) => (
        <li key={sheet.id}>
          <Link
            href={`/projects/${projectId}/sheets/${sheet.id}`}
            className="block overflow-hidden rounded-xl border border-border bg-card text-foreground no-underline hover:border-input hover:text-foreground"
          >
            <SheetThumb id={sheet.id} index={i} />
            <div className="flex flex-col gap-1.5 border-t border-border-subtle px-4 py-3.5">
              <span className="text-[15px] font-semibold">{sheet.name}</span>
              <span className="text-xs text-subtle-foreground">
                {updatedLabel(sheet.updated_at).replace("Updated", "Edited")} · {sheet.visual_count}{" "}
                {sheet.visual_count === 1 ? "visual" : "visuals"}
              </span>
              <span
                className={
                  sheet.link_audience === "off"
                    ? "w-fit rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-secondary-foreground"
                    : "w-fit rounded-full bg-success-surface px-2 py-0.5 text-xs font-medium text-success"
                }
              >
                {sheet.link_audience === "off" ? "Project members" : "View link on"}
              </span>
            </div>
          </Link>
        </li>
      ))}
    </ul>
  );
}

function NewSheetButton({ projectId, count }: { projectId: string; count: number }) {
  const router = useRouter();
  const create = useCreateSheet(projectId);
  return (
    <Button
      disabled={create.isPending}
      onClick={() =>
        create.mutate(
          { name: `Sheet ${count + 1}` },
          { onSuccess: (sheet) => router.push(`/projects/${projectId}/sheets/${sheet.id}`) },
        )
      }
    >
      + New sheet
    </Button>
  );
}
