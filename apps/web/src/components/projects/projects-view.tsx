"use client";

import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMemo } from "react";

import { CardGridSkeleton, ErrorState } from "@/components/shell/page-skeleton";
import { isSignedOutError } from "@/components/shell/auth-gate";
import { Button } from "@/components/ui/button";
import { pluralize } from "@/lib/format";
import { useProjects } from "@/lib/queries";
import { cn } from "@/lib/utils";

import { NewProjectDialog } from "./new-project-dialog";
import { ProjectCard } from "./project-card";

const FILTERS = [
  { id: "all", label: "All" },
  { id: "owned", label: "Owned by me" },
  { id: "shared", label: "Shared with me" },
] as const;
type Filter = (typeof FILTERS)[number]["id"];

export function ProjectsView() {
  const router = useRouter();
  const pathname = usePathname();
  const param = useSearchParams().get("filter");
  const filter: Filter = param === "owned" || param === "shared" ? param : "all";
  const projects = useProjects();

  const shown = useMemo(() => {
    const all = projects.data ?? [];
    if (filter === "owned") return all.filter((p) => p.owned_by_me);
    if (filter === "shared") return all.filter((p) => !p.owned_by_me);
    return all;
  }, [projects.data, filter]);

  const pick = (next: Filter) =>
    router.replace(next === "all" ? pathname : `${pathname}?filter=${next}`, { scroll: false });

  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div
          role="tablist"
          aria-label="Filter projects"
          className="flex gap-1 rounded-[10px] bg-[#eceef2] p-1"
        >
          {FILTERS.map((f) => (
            <button
              key={f.id}
              type="button"
              role="tab"
              aria-selected={filter === f.id}
              onClick={() => pick(f.id)}
              className={cn(
                "h-[34px] cursor-pointer rounded-[7px] px-3.5 text-[13px] font-medium",
                filter === f.id
                  ? "bg-card text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
        {projects.data && (
          <span className="text-[13px] text-subtle-foreground">
            {pluralize(shown.length, "project")}
          </span>
        )}
      </div>

      {projects.isPending || isSignedOutError(projects.error) ? (
        <CardGridSkeleton />
      ) : projects.error ? (
        <ErrorState
          title="Couldn't load projects"
          message={projects.error.message}
          action={
            <Button variant="outline" size="sm" onClick={() => void projects.refetch()}>
              Try again
            </Button>
          }
        />
      ) : shown.length === 0 ? (
        <EmptyState filter={filter} />
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,300px),1fr))] gap-4">
          {shown.map((project) => (
            <ProjectCard key={project.id} project={project} />
          ))}
        </div>
      )}
    </>
  );
}

function EmptyState({ filter }: { filter: Filter }) {
  const message =
    filter === "shared"
      ? "Nobody has shared a project with you yet."
      : "Projects keep their own sources, sheets and members. Create one to get started.";
  return (
    <div className="flex flex-col items-start gap-3 rounded-xl border border-dashed border-input bg-card p-8">
      <h2 className="text-[17px] font-semibold">
        {filter === "shared" ? "Nothing shared yet" : "No projects yet"}
      </h2>
      <p className="max-w-[520px] text-sm text-muted-foreground">{message}</p>
      {filter !== "shared" && <NewProjectDialog />}
    </div>
  );
}
