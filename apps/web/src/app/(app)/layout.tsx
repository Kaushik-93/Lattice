import { Suspense, type ReactNode } from "react";

import { AuthGate } from "@/components/shell/auth-gate";
import { PageSkeleton } from "@/components/shell/page-skeleton";
import { Sidebar, SidebarFallback } from "@/components/shell/sidebar";

/**
 * Signed-in shell. The layout itself is prerendered; the parts that read the URL sit
 * behind their own Suspense boundaries, so the static shell and skeletons paint first.
 */
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen flex-wrap items-stretch bg-background">
      <Suspense fallback={<SidebarFallback />}>
        <Sidebar />
      </Suspense>
      <main className="flex min-w-0 flex-[999_1_560px] flex-col gap-6 px-10 pt-8 pb-12 max-sm:px-5">
        <Suspense fallback={<PageSkeleton />}>
          <AuthGate>{children}</AuthGate>
        </Suspense>
      </main>
    </div>
  );
}
