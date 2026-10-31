import { GATEWAY_URL } from "@/lib/gateway";
import { perf } from "@/lib/perf/store";

let started = false;

/**
 * Starts the browser performance observers once per page load. Observers are passive
 * (the browser buffers entries off the main thread), so this costs nothing measurable.
 */
export function startPerfObservers(): void {
  if (started || typeof PerformanceObserver === "undefined") return;
  started = true;

  const navigation = performance.getEntriesByType("navigation")[0] as
    PerformanceNavigationTiming | undefined;
  if (navigation) {
    perf.vitals.ttfb = navigation.responseStart;
    if (navigation.domContentLoadedEventEnd > 0)
      perf.vitals.dcl = navigation.domContentLoadedEventEnd;
    if (navigation.loadEventEnd > 0) perf.vitals.load = navigation.loadEventEnd;
  }
  window.addEventListener(
    "load",
    () => {
      const nav = performance.getEntriesByType("navigation")[0] as
        PerformanceNavigationTiming | undefined;
      if (nav) {
        perf.vitals.dcl = nav.domContentLoadedEventEnd;
        perf.vitals.load = nav.loadEventEnd;
      }
    },
    { once: true },
  );

  observe("paint", (entry) => {
    if (entry.name === "first-contentful-paint") perf.vitals.fcp = entry.startTime;
  });
  observe("largest-contentful-paint", (entry) => {
    perf.vitals.lcp = entry.startTime;
  });
  // Main-thread tasks over 50 ms: the §3 budget says there should be none.
  observe("longtask", (entry) => perf.longTasks.push(entry.duration));
  observe("resource", (entry) => {
    const resource = entry as PerformanceResourceTiming;
    if (!resource.name.startsWith(GATEWAY_URL)) return;
    const url = new URL(resource.name);
    perf.requests.push({
      path: url.pathname,
      clientMs: resource.duration,
      // Readable cross-origin because the gateway sends Timing-Allow-Origin.
      server: (resource.serverTiming ?? []).map((t) => ({ name: t.name, ms: t.duration })),
      at: resource.startTime,
    });
  });
}

function observe(type: string, onEntry: (entry: PerformanceEntry) => void): void {
  try {
    new PerformanceObserver((list) => list.getEntries().forEach(onEntry)).observe({
      type,
      buffered: true,
    });
  } catch {
    // Entry type unsupported in this browser (e.g. longtask in Safari): skip it.
  }
}
