"use client";

import { useParams } from "next/navigation";

import { SheetsView } from "./sheet-view";

/** Reads the project from the URL (inside Suspense, as Next 16 requires). */
export function SheetsRoute() {
  const { projectId } = useParams<{ projectId: string }>();
  return <SheetsView projectId={projectId} />;
}
