import type { Metadata } from "next";
import { Suspense } from "react";

import { ProjectOverview, ProjectOverviewSkeleton } from "@/components/projects/project-overview";

export const metadata: Metadata = { title: "Project" };

export default function ProjectPage() {
  // The project id is request-time URL data, so the overview renders behind Suspense and
  // the prerendered shell shows a skeleton immediately.
  return (
    <Suspense fallback={<ProjectOverviewSkeleton />}>
      <ProjectOverview />
    </Suspense>
  );
}
