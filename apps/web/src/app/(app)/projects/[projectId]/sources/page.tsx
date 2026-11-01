import type { Metadata } from "next";
import { Suspense } from "react";

import { PageSkeleton } from "@/components/shell/page-skeleton";
import { SourcesView } from "@/components/sources/sources-view";

export const metadata: Metadata = { title: "Sources" };

export default function SourcesPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <SourcesView />
    </Suspense>
  );
}
