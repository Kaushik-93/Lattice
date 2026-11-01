import type { Metadata } from "next";
import { Suspense } from "react";

import { LogicView } from "@/components/logic/logic-view";
import { PageSkeleton } from "@/components/shell/page-skeleton";

export const metadata: Metadata = { title: "Business logic" };

export default function LogicPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <LogicView />
    </Suspense>
  );
}
