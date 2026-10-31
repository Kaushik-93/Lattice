import type { Metadata } from "next";
import { Suspense } from "react";

import { DraftFromQuestion } from "@/components/projects/draft-from-question";
import { NewProjectDialog } from "@/components/projects/new-project-dialog";
import { ProjectsView } from "@/components/projects/projects-view";
import { CardGridSkeleton } from "@/components/shell/page-skeleton";

export const metadata: Metadata = { title: "Projects" };

export default function ProjectsPage() {
  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">Projects</h1>
          <p className="max-w-[640px] text-[15px] text-muted-foreground">
            Each project has its own sources, sheets and members — on shared business logic.
          </p>
        </div>
        <NewProjectDialog />
      </header>
      <DraftFromQuestion />
      {/* The filter lives in the URL; everything above prerenders, the grid streams in. */}
      <Suspense fallback={<CardGridSkeleton />}>
        <ProjectsView />
      </Suspense>
    </>
  );
}
