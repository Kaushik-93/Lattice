"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Profiler, useState, type ProfilerOnRenderCallback, type ReactNode } from "react";

import { TooltipProvider } from "@/components/ui/tooltip";
import { perf } from "@/lib/perf/store";

// React only calls this in development and profiling builds; production skips it entirely.
const recordCommit: ProfilerOnRenderCallback = (id, phase, actualDuration) => {
  perf.commits.push({ id, phase, ms: actualDuration, at: performance.now() });
};

export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            // Refetch-on-focus storms are a classic source of surprise jank; refresh explicitly.
            refetchOnWindowFocus: false,
          },
        },
      }),
  );
  return (
    <QueryClientProvider client={client}>
      <TooltipProvider delayDuration={250}>
        <Profiler id="app" onRender={recordCommit}>
          {children}
        </Profiler>
      </TooltipProvider>
    </QueryClientProvider>
  );
}
