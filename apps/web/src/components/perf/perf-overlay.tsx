"use client";

import { useEffect, useState } from "react";

import { startPerfObservers } from "@/lib/perf/observe";
import { perf, percentile, type RequestSample } from "@/lib/perf/store";
import { useUiStore } from "@/lib/ui-store";
import { GatewaySocket } from "@/lib/ws";

/**
 * Dev performance overlay, toggled with Ctrl+Shift+P (or `?perf` in the URL). Shows page
 * vitals, main-thread health, the gateway WebSocket round trip, and the Server-Timing
 * breakdown of recent gateway requests next to what the browser observed.
 */
export function PerfOverlay() {
  const open = useUiStore((s) => s.perfOverlayOpen);
  const toggle = useUiStore((s) => s.togglePerfOverlay);
  const setOpen = useUiStore((s) => s.setPerfOverlayOpen);

  useEffect(() => {
    startPerfObservers();
    if (new URLSearchParams(window.location.search).has("perf")) setOpen(true);
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle, setOpen]);

  return open ? <OverlayPanel onClose={() => setOpen(false)} /> : null;
}

interface Snapshot {
  vitals: typeof perf.vitals;
  requests: RequestSample[];
  rtt: number[];
  longTasks: number[];
  commits: number[];
}

function snapshot(): Snapshot {
  return {
    vitals: { ...perf.vitals },
    requests: perf.requests.toArray().reverse().slice(0, 8),
    rtt: perf.rtt.toArray(),
    longTasks: perf.longTasks.toArray(),
    commits: perf.commits.toArray().map((c) => c.ms),
  };
}

function OverlayPanel({ onClose }: { onClose: () => void }) {
  const [snap, setSnap] = useState(snapshot);
  const [fps, setFps] = useState<number>();
  const [socket, setSocket] = useState<"connecting" | "open" | "closed">("connecting");

  useEffect(() => {
    const id = setInterval(() => setSnap(snapshot()), 250);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    let frames = 0;
    let last = performance.now();
    let raf = requestAnimationFrame(function loop(now) {
      frames += 1;
      if (now - last >= 1000) {
        setFps(Math.round((frames * 1000) / (now - last)));
        frames = 0;
        last = now;
      }
      raf = requestAnimationFrame(loop);
    });
    return () => cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    const ws = new GatewaySocket((message) => {
      if (message.type === "pong") perf.rtt.push(performance.now() - message.client_time_ms);
    }, setSocket);
    ws.connect();
    const id = setInterval(() => ws.ping(), 1000);
    return () => {
      clearInterval(id);
      ws.close();
    };
  }, []);

  const lastRtt = snap.rtt.at(-1);
  const maxLongTask = snap.longTasks.length ? Math.max(...snap.longTasks) : undefined;

  return (
    <aside
      aria-label="Performance overlay"
      className="fixed right-3 bottom-3 z-[1000] w-[380px] max-w-[calc(100vw-24px)] rounded-xl border border-white/10 bg-[#0f1424]/95 p-3 font-mono text-[11px] leading-relaxed text-[#e7eaf3] shadow-2xl backdrop-blur"
    >
      <header className="mb-2 flex items-center justify-between">
        <span className="font-sans text-xs font-semibold">Performance · Ctrl+Shift+P</span>
        <button
          type="button"
          onClick={onClose}
          className="rounded px-1.5 text-[#b4bbcc] hover:bg-white/10"
          aria-label="Close performance overlay"
        >
          ✕
        </button>
      </header>

      <Section title="Page load">
        <Metric label="TTFB" value={snap.vitals.ttfb} />
        <Metric label="FCP" value={snap.vitals.fcp} budget={1200} />
        <Metric label="LCP" value={snap.vitals.lcp} budget={1200} />
        <Metric label="DOMContentLoaded" value={snap.vitals.dcl} />
      </Section>

      <Section title="Main thread">
        <Row
          label="FPS"
          value={fps === undefined ? "…" : String(fps)}
          bad={fps !== undefined && fps < 55}
        />
        <Row
          label="Long tasks (>50 ms)"
          value={String(snap.longTasks.length)}
          bad={snap.longTasks.length > 0}
        />
        <Metric label="Longest task" value={maxLongTask} budget={50} />
        <Row
          label="React commit p95"
          value={
            snap.commits.length
              ? `${fmt(percentile(snap.commits, 95))} ms`
              : "n/a in production build"
          }
        />
      </Section>

      <Section title={`Gateway WebSocket · ${socket}`}>
        <Metric label="RTT (last)" value={lastRtt} />
        <Metric label="RTT p95" value={percentile(snap.rtt, 95)} />
      </Section>

      <Section title="Gateway requests (browser total · server phases)">
        {snap.requests.length === 0 ? (
          <p className="text-[#8e97ad]">No requests yet.</p>
        ) : (
          snap.requests.map((r) => (
            <div
              key={`${r.at}-${r.path}`}
              className="flex justify-between gap-2 border-t border-white/5 py-0.5"
            >
              <span className="truncate text-[#b4bbcc]">{r.path}</span>
              <span className="shrink-0">
                {fmt(r.clientMs)} ms
                <span className="text-[#8e97ad]">
                  {r.server.length > 0 &&
                    ` · ${r.server.map((p) => `${p.name} ${fmt(p.ms)}`).join(" ")}`}
                </span>
              </span>
            </div>
          ))
        )}
      </Section>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-2 last:mb-0">
      <h3 className="mb-0.5 font-sans text-[10px] font-semibold tracking-wider text-[#8e97ad] uppercase">
        {title}
      </h3>
      {children}
    </section>
  );
}

function Row({ label, value, bad = false }: { label: string; value: string; bad?: boolean }) {
  return (
    <div className="flex justify-between">
      <span className="text-[#b4bbcc]">{label}</span>
      <span className={bad ? "text-[#ff8a80]" : undefined}>{value}</span>
    </div>
  );
}

function Metric({
  label,
  value,
  budget,
}: {
  label: string;
  value: number | undefined;
  budget?: number;
}) {
  return (
    <Row
      label={budget ? `${label} (budget ${budget} ms)` : label}
      value={value === undefined ? "…" : `${fmt(value)} ms`}
      bad={budget !== undefined && value !== undefined && value > budget}
    />
  );
}

function fmt(ms: number | undefined): string {
  if (ms === undefined) return "…";
  return ms < 10 ? ms.toFixed(2) : ms < 100 ? ms.toFixed(1) : Math.round(ms).toString();
}
