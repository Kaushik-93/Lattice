import type { Metadata } from "next";
import { Suspense } from "react";

import { SourceExplorer } from "@/components/explorer/source-explorer";
import { PageSkeleton } from "@/components/shell/page-skeleton";

export const metadata: Metadata = { title: "Explore source" };

export default function SourcePage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <SourceExplorer />
    </Suspense>
  );
}
