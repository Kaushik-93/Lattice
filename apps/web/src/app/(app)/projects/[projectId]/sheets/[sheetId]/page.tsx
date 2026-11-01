import type { Metadata } from "next";
import { Suspense } from "react";

import { PageSkeleton } from "@/components/shell/page-skeleton";
import { SheetsRoute } from "@/components/sheets/sheets-route";

export const metadata: Metadata = { title: "Sheets" };

export default function SheetsPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <SheetsRoute />
    </Suspense>
  );
}
