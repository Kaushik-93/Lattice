import type { Metadata } from "next";
import { Suspense } from "react";

import { SharedSheet } from "@/components/sheets/shared-sheet";

export const metadata: Metadata = { title: "Shared sheet", robots: { index: false } };

/** A sheet opened through its share link: no sign-in for public links, no project shell. */
export default function SharedSheetPage() {
  return (
    <Suspense>
      <SharedSheet />
    </Suspense>
  );
}
