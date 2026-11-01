"use client";

import type { Visual } from "@lattice/shared-types";
import type { Table } from "apache-arrow";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import createREGL from "regl";

import { compute, latestOnly } from "@/lib/compute/client";
import { OTHER, PALETTE, SCATTER_SLOTS } from "@/lib/sheets/charts";
import { niceTicks, preparePoints, timeTicks, type Points } from "@/lib/sheets/points";
import { formatValue } from "@/lib/sheets/values";

/** Plot-area insets for the axes, in CSS pixels. */
const INSET = { left: 56, right: 12, top: 8, bottom: 26 };
/** Hover radius in CSS pixels. */
const HOVER_PX = 6;
const MIN_SPAN = 1e-5;

type View = { x0: number; x1: number; y0: number; y1: number };
const FULL: View = { x0: -0.02, x1: 1.02, y0: -0.03, y1: 1.03 };

function rgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const VERT = `
precision highp float;
attribute vec2 position;
attribute float color;
uniform vec4 view;
uniform vec4 plot;
uniform vec2 canvas;
uniform float size;
varying float vColor;
void main() {
  vec2 t = (position - view.xy) / (view.zw - view.xy);
  vec2 px = plot.xy + t * plot.zw;
  gl_Position = vec4(px / canvas * 2.0 - 1.0, 0.0, 1.0);
  gl_PointSize = size;
  vColor = color;
}`;

const FRAG = `
precision mediump float;
varying float vColor;
uniform vec3 c0;
uniform vec3 c1;
uniform vec3 c2;
uniform vec3 c3;
uniform float alpha;
void main() {
  vec2 d = gl_PointCoord - 0.5;
  if (dot(d, d) > 0.25) discard;
  vec3 c = vColor < 0.5 ? c0 : vColor < 1.5 ? c1 : vColor < 2.5 ? c2 : c3;
  gl_FragColor = vec4(c * alpha, alpha);
}`;

/**
 * A scatter plot drawn with WebGL (spec §2.1: series beyond ~200k points). Points live on the
 * GPU once; panning and zooming only change two uniforms, so a million points stay at 60 fps.
 * Hover and brush counts use the point index in the compute worker.
 */
export const ScatterGl = memo(function ScatterGl({
  visual,
  table,
}: {
  visual: Visual;
  table: Table;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<HTMLCanvasElement>(null);
  const axesRef = useRef<HTMLCanvasElement>(null);
  const points = useMemo(() => preparePoints(table), [table]);
  const [hover, setHover] = useState<{ i: number; left: number; top: number } | null>(null);
  const [brush, setBrush] = useState<{
    x: number;
    y: number;
    w: number;
    h: number;
    count: number | null;
  } | null>(null);
  const indexKey = visual.id;

  useEffect(() => {
    const wrap = wrapRef.current;
    const glCanvas = glRef.current;
    const axesCanvas = axesRef.current;
    if (!wrap || !glCanvas || !axesCanvas || points.count === 0) return;

    const regl = createREGL({
      canvas: glCanvas,
      attributes: { antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: false },
    });
    const position = regl.buffer(points.positions);
    const color = regl.buffer(points.colors);
    const colors = [...PALETTE.slice(0, SCATTER_SLOTS), OTHER].map(rgb);
    const draw = regl({
      vert: VERT,
      frag: FRAG,
      attributes: { position, color },
      uniforms: {
        view: regl.prop<{ view: number[] }, "view">("view"),
        plot: regl.prop<{ plot: number[] }, "plot">("plot"),
        canvas: regl.prop<{ canvas: number[] }, "canvas">("canvas"),
        size: regl.prop<{ size: number }, "size">("size"),
        alpha: regl.prop<{ alpha: number }, "alpha">("alpha"),
        c0: colors[0] ?? [0, 0, 0],
        c1: colors[1] ?? [0, 0, 0],
        c2: colors[2] ?? [0, 0, 0],
        c3: colors[3] ?? [0, 0, 0],
      },
      count: points.count,
      primitive: "points",
      depth: { enable: false },
      // Points panned past the plot area stay off the axes.
      scissor: {
        enable: true,
        box: regl.prop<{ box: { x: number; y: number; width: number; height: number } }, "box">(
          "box",
        ),
      },
      blend: {
        enable: true,
        func: {
          srcRGB: 1,
          srcAlpha: 1,
          dstRGB: "one minus src alpha",
          dstAlpha: "one minus src alpha",
        },
      },
    });

    // The worker gets its own copy of the coordinates to index (transferred, not shared).
    const xs = new Float32Array(points.count);
    const ys = new Float32Array(points.count);
    for (let i = 0; i < points.count; i++) {
      xs[i] = points.positions[i * 2] ?? 0;
      ys[i] = points.positions[i * 2 + 1] ?? 0;
    }
    void compute().call({ op: "index", key: indexKey, xs, ys });

    let view: View = { ...FULL };
    let size = { w: 1, h: 1, dpr: 1 };
    let frame = 0;
    const plotW = () => size.w - INSET.left - INSET.right;
    const plotH = () => size.h - INSET.top - INSET.bottom;

    const render = () => {
      frame = 0;
      const { w, h, dpr } = size;
      regl.poll();
      regl.clear({ color: [0, 0, 0, 0] });
      const dense = points.count > 200_000;
      draw({
        view: [view.x0, view.y0, view.x1, view.y1],
        plot: [INSET.left * dpr, INSET.bottom * dpr, plotW() * dpr, plotH() * dpr],
        canvas: [w * dpr, h * dpr],
        size: (dense ? 2 : 4) * dpr,
        alpha: dense ? 0.35 : 0.8,
        box: {
          x: Math.round(INSET.left * dpr),
          y: Math.round(INSET.bottom * dpr),
          width: Math.round(plotW() * dpr),
          height: Math.round(plotH() * dpr),
        },
      });
      drawAxes(axesCanvas, size, view, points);
      wrap.dataset.frameAt = String(performance.now());
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(render);
    };

    const resize = () => {
      const rect = wrap.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      size = { w: Math.max(1, rect.width), h: Math.max(1, rect.height), dpr };
      for (const c of [glCanvas, axesCanvas]) {
        c.width = Math.round(size.w * dpr);
        c.height = Math.round(size.h * dpr);
      }
      schedule();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(wrap);
    resize();

    /** Pointer position → normalised data coordinates. */
    const toData = (clientX: number, clientY: number) => {
      const rect = wrap.getBoundingClientRect();
      const tx = (clientX - rect.left - INSET.left) / plotW();
      const ty = 1 - (clientY - rect.top - INSET.top) / plotH();
      return {
        x: view.x0 + tx * (view.x1 - view.x0),
        y: view.y0 + ty * (view.y1 - view.y0),
        tx,
        ty,
      };
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const { x, y } = toData(e.clientX, e.clientY);
      const k = Math.exp(e.deltaY * 0.0015);
      const span = (a: number, b: number, c: number) => {
        const lo = c - (c - a) * k;
        const hi = c + (b - c) * k;
        return hi - lo < MIN_SPAN ? [a, b] : [lo, hi];
      };
      const [x0, x1] = span(view.x0, view.x1, x);
      const [y0, y1] = e.shiftKey ? [view.y0, view.y1] : span(view.y0, view.y1, y);
      view = { x0: x0 ?? view.x0, x1: x1 ?? view.x1, y0: y0 ?? view.y0, y1: y1 ?? view.y1 };
      setHover(null);
      schedule();
    };

    let drag: { x: number; y: number; view: View; brush: boolean } | null = null;
    const nearest = latestOnly(
      (x: number, y: number, rx: number, ry: number) =>
        compute().call({ op: "nearest", key: indexKey, x, y, rx, ry }),
      (i: number) =>
        setHover((h) =>
          i < 0 ? null : h && h.i === i ? h : { i, left: lastPointer.x, top: lastPointer.y },
        ),
    );
    const count = latestOnly(
      (x0: number, y0: number, x1: number, y1: number) =>
        compute().call({ op: "count", key: indexKey, x0, y0, x1, y1 }),
      (n: number) => setBrush((b) => (b ? { ...b, count: n } : b)),
    );
    const lastPointer = { x: 0, y: 0 };

    const onDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      wrap.setPointerCapture(e.pointerId);
      drag = { x: e.clientX, y: e.clientY, view: { ...view }, brush: e.shiftKey };
      setHover(null);
    };
    const onMove = (e: PointerEvent) => {
      const rect = wrap.getBoundingClientRect();
      lastPointer.x = e.clientX - rect.left;
      lastPointer.y = e.clientY - rect.top;
      if (!drag) {
        const { x, y, tx, ty } = toData(e.clientX, e.clientY);
        if (tx < 0 || tx > 1 || ty < 0 || ty > 1) return setHover(null);
        nearest(
          x,
          y,
          (HOVER_PX / plotW()) * (view.x1 - view.x0),
          (HOVER_PX / plotH()) * (view.y1 - view.y0),
        );
        return;
      }
      if (drag.brush) {
        const x = Math.min(drag.x, e.clientX) - rect.left;
        const y = Math.min(drag.y, e.clientY) - rect.top;
        setBrush((b) => ({
          x,
          y,
          w: Math.abs(e.clientX - drag!.x),
          h: Math.abs(e.clientY - drag!.y),
          count: b?.count ?? null,
        }));
        const a = toData(drag.x, drag.y);
        const b = toData(e.clientX, e.clientY);
        count(a.x, a.y, b.x, b.y);
        return;
      }
      const dx = ((e.clientX - drag.x) / plotW()) * (drag.view.x1 - drag.view.x0);
      const dy = ((e.clientY - drag.y) / plotH()) * (drag.view.y1 - drag.view.y0);
      view = {
        x0: drag.view.x0 - dx,
        x1: drag.view.x1 - dx,
        y0: drag.view.y0 + dy,
        y1: drag.view.y1 + dy,
      };
      schedule();
    };
    const onUp = (e: PointerEvent) => {
      if (drag?.brush) {
        // Brush to zoom: the selected rectangle becomes the view.
        const a = toData(drag.x, drag.y);
        const b = toData(e.clientX, e.clientY);
        if (Math.abs(b.x - a.x) > MIN_SPAN && Math.abs(b.y - a.y) > MIN_SPAN) {
          view = {
            x0: Math.min(a.x, b.x),
            x1: Math.max(a.x, b.x),
            y0: Math.min(a.y, b.y),
            y1: Math.max(a.y, b.y),
          };
          schedule();
        }
        setBrush(null);
      }
      drag = null;
    };
    const onLeave = () => setHover(null);
    const onDouble = () => {
      view = { ...FULL };
      schedule();
    };
    wrap.addEventListener("wheel", onWheel, { passive: false });
    wrap.addEventListener("pointerdown", onDown);
    wrap.addEventListener("pointermove", onMove);
    wrap.addEventListener("pointerup", onUp);
    wrap.addEventListener("pointerleave", onLeave);
    wrap.addEventListener("dblclick", onDouble);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      wrap.removeEventListener("wheel", onWheel);
      wrap.removeEventListener("pointerdown", onDown);
      wrap.removeEventListener("pointermove", onMove);
      wrap.removeEventListener("pointerup", onUp);
      wrap.removeEventListener("pointerleave", onLeave);
      wrap.removeEventListener("dblclick", onDouble);
      void compute().call({ op: "drop", key: indexKey });
      regl.destroy();
    };
  }, [points, indexKey]);

  const hovered = hover ? describe(points, table, hover.i) : null;
  return (
    <div className="flex size-full flex-col gap-1.5">
      {points.categories.length > 0 && (
        <ul
          className="flex flex-wrap justify-end gap-3 text-xs text-secondary-foreground"
          aria-label="Legend"
        >
          {points.categories.map((name, i) => (
            <li key={name} className="flex items-center gap-1.5">
              <span
                className="size-2 rounded-full"
                style={{ background: i < SCATTER_SLOTS ? PALETTE[i] : OTHER }}
              />
              {name}
            </li>
          ))}
        </ul>
      )}
      <div
        ref={wrapRef}
        data-testid="scatter-gl"
        data-points={points.count}
        className="relative min-h-0 flex-1 cursor-crosshair touch-none select-none"
        role="img"
        aria-label={`${visual.spec.title}: ${points.count.toLocaleString("en-US")} points. Scroll to zoom, drag to pan, shift-drag to zoom into an area, double-click to reset.`}
      >
        {/* Grid and labels under the points. */}
        <canvas ref={axesRef} className="pointer-events-none absolute inset-0 size-full" />
        <canvas ref={glRef} className="absolute inset-0 size-full" />
        {brush && (
          <div
            className="pointer-events-none absolute border border-primary bg-primary/10"
            style={{ left: brush.x, top: brush.y, width: brush.w, height: brush.h }}
          >
            {brush.count !== null && (
              <span className="absolute -top-6 left-0 rounded bg-foreground px-1.5 py-0.5 text-[11px] whitespace-nowrap text-background">
                {brush.count.toLocaleString("en-US")} points
              </span>
            )}
          </div>
        )}
        {hover && hovered && (
          <div
            className="pointer-events-none absolute z-10 rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs shadow-md"
            style={{ left: hover.left + 12, top: hover.top + 12 }}
          >
            {hovered.map(([k, v]) => (
              <div key={k} className="flex gap-3">
                <span className="text-muted-foreground">{k}</span>
                <span className="ml-auto font-mono tabular-nums">{v}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});

function describe(points: Points, table: Table, i: number): [string, string][] {
  const fields = table.schema.fields;
  return fields.map((field, c) => {
    const v = table.getChildAt(c)?.get(i) as unknown;
    if (c === 0 && points.xTime && typeof v === "number") {
      return [field.name, new Date(v).toISOString().slice(0, 10)];
    }
    if (typeof v === "number" || typeof v === "bigint")
      return [field.name, formatValue(Number(v), undefined, { compact: false })];
    return [field.name, String(v ?? "")];
  });
}

function drawAxes(
  canvas: HTMLCanvasElement,
  size: { w: number; h: number; dpr: number },
  view: View,
  points: Points,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const { w, h, dpr } = size;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const plotW = w - INSET.left - INSET.right;
  const plotH = h - INSET.top - INSET.bottom;
  const [dx0, dx1] = points.xRange;
  const [dy0, dy1] = points.yRange;
  const xData = (n: number) => dx0 + n * (dx1 - dx0);
  const yData = (n: number) => dy0 + n * (dy1 - dy0);
  const px = (data: number) =>
    INSET.left + (((data - dx0) / (dx1 - dx0) - view.x0) / (view.x1 - view.x0)) * plotW;
  const py = (data: number) =>
    INSET.top + (1 - ((data - dy0) / (dy1 - dy0) - view.y0) / (view.y1 - view.y0)) * plotH;

  ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
  ctx.fillStyle = "#556070";
  ctx.strokeStyle = "#eef0f3";
  ctx.lineWidth = 1;

  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (const t of niceTicks(yData(view.y0), yData(view.y1), Math.max(2, Math.floor(plotH / 48)))) {
    const y = Math.round(py(t)) + 0.5;
    ctx.beginPath();
    ctx.moveTo(INSET.left, y);
    ctx.lineTo(INSET.left + plotW, y);
    ctx.stroke();
    ctx.fillText(formatValue(t, undefined), INSET.left - 8, y);
  }
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const target = Math.max(2, Math.floor(plotW / 110));
  const ticks = points.xTime
    ? timeTicks(xData(view.x0) * points.xScale, xData(view.x1) * points.xScale, target).map(
        (t) => ({
          at: t.at / points.xScale,
          label: t.label,
        }),
      )
    : niceTicks(xData(view.x0), xData(view.x1), target).map((t) => ({
        at: t,
        label: formatValue(t, undefined),
      }));
  for (const t of ticks) {
    const x = px(t.at);
    if (x < INSET.left || x > INSET.left + plotW) continue;
    ctx.fillText(t.label, x, INSET.top + plotH + 8);
  }
  ctx.strokeStyle = "#e4e7ec";
  ctx.beginPath();
  ctx.moveTo(INSET.left, INSET.top + plotH + 0.5);
  ctx.lineTo(INSET.left + plotW, INSET.top + plotH + 0.5);
  ctx.stroke();
}
