"use client";

import type { ProjectSummary } from "@lattice/shared-types";
import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { memo } from "react";

import { capitalize, updatedLabel } from "@/lib/format";
import { prefetchProject } from "@/lib/queries";

import { ProjectTile } from "./project-tile";

export const ProjectCard = memo(function ProjectCard({ project }: { project: ProjectSummary }) {
  const client = useQueryClient();
  const warm = () => prefetchProject(client, project.id);
  const role = project.owned_by_me ? "Owner" : capitalize(project.my_role);
  const extraMembers = project.member_count - project.member_initials.length;

  return (
    <Link
      href={`/projects/${project.id}`}
      onMouseEnter={warm}
      onFocus={warm}
      className="flex flex-col gap-3.5 rounded-xl border border-border bg-card p-[18px] text-foreground no-underline transition-colors hover:border-[#a5abf0] hover:text-foreground"
    >
      <div className="flex items-center gap-3">
        <ProjectTile name={project.name} color={project.color} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate text-base font-semibold">{project.name}</span>
          <span className="text-xs text-subtle-foreground">{updatedLabel(project.updated_at)}</span>
        </div>
        <span className="rounded-full bg-secondary px-[9px] py-[3px] text-xs font-medium text-secondary-foreground">
          {role}
        </span>
      </div>
      <p className="line-clamp-2 min-h-[42px] text-sm leading-normal text-muted-foreground">
        {project.description || "No description yet."}
      </p>
      <div className="flex gap-4 text-[13px] text-secondary-foreground">
        <span>
          <strong className="font-semibold">{project.source_count}</strong> sources
        </span>
        <span>
          <strong className="font-semibold">{project.sheet_count}</strong> sheets
        </span>
        <span>
          <strong className="font-semibold">{project.definition_count}</strong> definitions
        </span>
      </div>
      <div className="flex items-center border-t border-border-subtle pt-3">
        <div className="flex">
          {project.member_initials.map((initials, i) => (
            <span
              key={`${initials}-${i}`}
              className="-mr-1.5 flex size-7 items-center justify-center rounded-full border-2 border-card bg-avatar text-[10px] font-semibold text-avatar-foreground"
            >
              {initials}
            </span>
          ))}
        </div>
        {extraMembers > 0 && (
          <span className="ml-3 self-center text-xs text-subtle-foreground">+{extraMembers}</span>
        )}
      </div>
    </Link>
  );
});
