import type { Metadata } from "next";
import { Suspense } from "react";

import { SharingView } from "@/components/sharing/sharing-view";
import { PageSkeleton } from "@/components/shell/page-skeleton";

export const metadata: Metadata = { title: "Members & sharing" };

export default function SharingPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <SharingView />
    </Suspense>
  );
}
