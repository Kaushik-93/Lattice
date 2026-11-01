"use client";

import type { ProjectMember } from "@lattice/shared-types";
import { Share2 } from "lucide-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import type { ReactNode } from "react";

import { SparkleIcon } from "@/components/auth/auth-aside";
import { ErrorState } from "@/components/shell/page-skeleton";
import { isSignedOutError } from "@/components/shell/auth-gate";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { SourceBadge, statusDot, statusLabel } from "@/components/sources/source-meta";
import { capitalize } from "@/lib/format";
import { useProject, useSources } from "@/lib/queries";

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
          <Later label="Sharing arrives with sheets">
            <Button variant="outline" disabled>
              <Share2 /> Share project
            </Button>
          </Later>
          <Later label="Sheets arrive in the next phase">
            <Button disabled>+ New sheet</Button>
          </Later>
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
        <Empty>
          No sheets yet. Sheets — grids of charts, tables and filters — are coming next.
        </Empty>
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

function Later({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0} className="flex rounded-[10px]">
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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
