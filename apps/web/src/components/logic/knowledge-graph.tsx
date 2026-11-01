"use client";

import type { KnowledgeGraph as Graph, NoteKind, NoteState } from "@lattice/shared-types";
import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import { Maximize, Minus, Plus } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";

import { titleKey } from "@/lib/knowledge/links";
import { GHOST, nodeLook, stateSuffix, type NodeLook } from "@/lib/knowledge/style";

const NODE_H = 32;
const FONT = '600 13px ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif';
const SUFFIX_FONT = "500 11px ui-sans-serif, system-ui, sans-serif";

interface Node extends SimulationNodeDatum {
  id: string;
  title: string;
  kind: NoteKind | null;
  state: NoteState | null;
  look: NodeLook;
  suffix: string | null;
  w: number;
}

interface Link extends SimulationLinkDatum<Node> {
  source: string | Node;
  target: string | Node;
  tentative: boolean;
}

let measurer: CanvasRenderingContext2D | null = null;
function textWidth(text: string, font: string): number {
  measurer ??= document.createElement("canvas").getContext("2d");
  if (!measurer) return text.length * 7.5;
  measurer.font = font;
  return measurer.measureText(text).width;
}

/** Notes, plus a ghost node per unresolved link title (as Obsidian draws them). */
function build(graph: Graph): { nodes: Node[]; links: Link[] } {
  const nodes: Node[] = graph.nodes.map((n) => {
    const suffix = stateSuffix(n.state);
    return {
      id: n.id,
      title: n.title,
      kind: n.kind,
      state: n.state,
      look: nodeLook(n.kind, n.state),
      suffix,
      w: textWidth(n.title, FONT) + (suffix ? textWidth(suffix, SUFFIX_FONT) + 8 : 0) + 28,
    };
  });
  const tentative = new Set(
    graph.nodes.filter((n) => n.state === "draft" || n.state === "pending").map((n) => n.id),
  );
  const links: Link[] = graph.edges.map((e) => ({
    source: e.from,
    target: e.to,
    tentative: tentative.has(e.from) || tentative.has(e.to),
  }));
  const ghosts = new Map<string, Node>();
  for (const u of graph.unresolved) {
    const id = `ghost:${titleKey(u.title)}`;
    if (!ghosts.has(id)) {
      ghosts.set(id, {
        id,
        title: u.title,
        kind: null,
        state: null,
        look: GHOST,
        suffix: null,
        w: textWidth(u.title, FONT) + 28,
      });
    }
    links.push({ source: u.from, target: id, tentative: true });
  }
  return { nodes: [...nodes, ...ghosts.values()], links };
}

/** Ids within `depth` links of `id`, either direction. */
export function neighbourhood(
  links: { source: string; target: string }[],
  id: string,
  depth: number,
): Set<string> {
  const seen = new Set([id]);
  let frontier = [id];
  for (let d = 0; d < depth; d++) {
    const next: string[] = [];
    for (const l of links) {
      for (const [a, b] of [
        [l.source, l.target],
        [l.target, l.source],
      ] as const) {
        if (frontier.includes(a) && !seen.has(b)) {
          seen.add(b);
          next.push(b);
        }
      }
    }
    frontier = next;
  }
  return seen;
}

const idOf = (end: string | Node) => (typeof end === "string" ? end : end.id);

interface Props {
  graph: Graph;
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** A ghost (unresolved link) was clicked: offer to create the note. */
  onGhost: (title: string) => void;
  search: string;
  /** Show only the selection's neighbourhood (Obsidian's local graph). */
  local: boolean;
}

/**
 * The knowledge graph as a force-directed layout. Positions live outside React: the
 * simulation writes them straight into the SVG each tick, so a layout settling or a node
 * being dragged never re-renders the component.
 */
export const KnowledgeGraph = memo(function KnowledgeGraph({
  graph,
  selected,
  onSelect,
  onGhost,
  search,
  local,
}: Props) {
  const svgRef = useRef<SVGSVGElement>(null);
  const layerRef = useRef<SVGGElement>(null);
  const positions = useRef(new Map<string, { x: number; y: number }>());
  const view = useRef({ k: 1, x: 0, y: 0 });
  /** Set once the user pans or zooms: the layout stops re-fitting itself. */
  const userMoved = useRef(false);
  const sim = useRef<Simulation<Node, Link> | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);

  const full = useMemo(() => build(graph), [graph]);
  const plainLinks = useMemo(
    () => full.links.map((l) => ({ source: idOf(l.source), target: idOf(l.target) })),
    [full],
  );
  const { nodes, links } = useMemo(() => {
    if (!local || !selected) return full;
    const keep = neighbourhood(plainLinks, selected, 2);
    return {
      nodes: full.nodes.filter((n) => keep.has(n.id)),
      links: full.links.filter((l) => keep.has(idOf(l.source)) && keep.has(idOf(l.target))),
    };
  }, [full, plainLinks, local, selected]);

  // What to emphasise: the hovered node's neighbours, or search matches.
  const focus = useMemo(() => {
    if (hovered) return neighbourhood(plainLinks, hovered, 1);
    const q = search.trim().toLowerCase();
    if (q) return new Set(nodes.filter((n) => n.title.toLowerCase().includes(q)).map((n) => n.id));
    return null;
  }, [hovered, search, nodes, plainLinks]);

  const applyView = () => {
    const { k, x, y } = view.current;
    layerRef.current?.setAttribute("transform", `translate(${x},${y}) scale(${k})`);
  };

  const fit = () => {
    const svg = svgRef.current;
    if (!svg || nodes.length === 0) return;
    const xs = nodes.flatMap((n) => [(n.x ?? 0) - n.w / 2, (n.x ?? 0) + n.w / 2]);
    const ys = nodes.flatMap((n) => [(n.y ?? 0) - NODE_H / 2, (n.y ?? 0) + NODE_H / 2]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const { width, height } = svg.getBoundingClientRect();
    const k = Math.min(
      1.6,
      Math.max(0.2, Math.min(width / (x1 - x0 + 80), height / (y1 - y0 + 80))),
    );
    view.current = { k, x: width / 2 - ((x0 + x1) / 2) * k, y: height / 2 - ((y0 + y1) / 2) * k };
    applyView();
  };
  const zoomBy = (factor: number) => {
    userMoved.current = true;
    const svg = svgRef.current;
    if (!svg) return;
    const { width, height } = svg.getBoundingClientRect();
    const v = view.current;
    const k = Math.min(3, Math.max(0.15, v.k * factor));
    view.current = {
      k,
      x: width / 2 - ((width / 2 - v.x) / v.k) * k,
      y: height / 2 - ((height / 2 - v.y) / v.k) * k,
    };
    applyView();
  };

  // The simulation: rebuilt when the visible graph changes, starting from known positions.
  useEffect(() => {
    const layer = layerRef.current;
    if (!layer) return;
    for (const n of nodes) {
      const known = positions.current.get(n.id);
      if (known) {
        n.x = known.x;
        n.y = known.y;
      }
    }
    const simulation = forceSimulation<Node, Link>(nodes)
      .force(
        "link",
        forceLink<Node, Link>(links)
          .id((d) => d.id)
          .distance(110)
          .strength(0.35),
      )
      .force("charge", forceManyBody<Node>().strength(-420))
      .force("collide", forceCollide<Node>((d) => d.w / 2 + 10).strength(0.9))
      .force("center", forceCenter(0, 0))
      .force("x", forceX<Node>().strength(0.03))
      .force("y", forceY<Node>().strength(0.05))
      .alpha(positions.current.size ? 0.4 : 1);
    sim.current = simulation;
    let fitted = positions.current.size > 0;
    const els = new Map<string, SVGGElement>();
    layer
      .querySelectorAll<SVGGElement>("g[data-node]")
      .forEach((el) => els.set(el.dataset.node ?? "", el));
    const lines = Array.from(layer.querySelectorAll<SVGLineElement>("line[data-link]"));
    simulation.on("tick", () => {
      for (const n of nodes) {
        positions.current.set(n.id, { x: n.x ?? 0, y: n.y ?? 0 });
        els.get(n.id)?.setAttribute("transform", `translate(${n.x ?? 0},${n.y ?? 0})`);
      }
      links.forEach((l, i) => {
        const [s, t] = [l.source as Node, l.target as Node];
        const line = lines[i];
        if (!line) return;
        line.setAttribute("x1", String(s.x ?? 0));
        line.setAttribute("y1", String(s.y ?? 0));
        line.setAttribute("x2", String(t.x ?? 0));
        line.setAttribute("y2", String(t.y ?? 0));
      });
      if (!fitted && simulation.alpha() < 0.25) {
        fitted = true;
        fit();
      }
    });
    // Fit again once the layout has settled, unless the user has taken over the view.
    simulation.on("end", () => {
      if (!userMoved.current) fit();
    });
    return () => {
      simulation.stop();
    };
    // fit reads the latest nodes through the closure.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, links]);

  // Pan by dragging the background, zoom with the wheel around the pointer.
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    let pan: { x: number; y: number; vx: number; vy: number } | null = null;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      userMoved.current = true;
      const rect = svg.getBoundingClientRect();
      const [px, py] = [e.clientX - rect.left, e.clientY - rect.top];
      const v = view.current;
      const k = Math.min(3, Math.max(0.15, v.k * Math.exp(-e.deltaY * 0.0015)));
      view.current = { k, x: px - ((px - v.x) / v.k) * k, y: py - ((py - v.y) / v.k) * k };
      applyView();
    };
    const onDown = (e: PointerEvent) => {
      if ((e.target as Element).closest("g[data-node]")) return;
      pan = { x: e.clientX, y: e.clientY, vx: view.current.x, vy: view.current.y };
      userMoved.current = true;
      svg.setPointerCapture(e.pointerId);
    };
    const onMove = (e: PointerEvent) => {
      if (!pan) return;
      view.current = {
        ...view.current,
        x: pan.vx + e.clientX - pan.x,
        y: pan.vy + e.clientY - pan.y,
      };
      applyView();
    };
    const onUp = (e: PointerEvent) => {
      if (pan && Math.hypot(e.clientX - pan.x, e.clientY - pan.y) < 4) onSelect(null);
      pan = null;
    };
    svg.addEventListener("wheel", onWheel, { passive: false });
    svg.addEventListener("pointerdown", onDown);
    svg.addEventListener("pointermove", onMove);
    svg.addEventListener("pointerup", onUp);
    return () => {
      svg.removeEventListener("wheel", onWheel);
      svg.removeEventListener("pointerdown", onDown);
      svg.removeEventListener("pointermove", onMove);
      svg.removeEventListener("pointerup", onUp);
    };
  }, [onSelect]);

  /** Dragging a node pins it while held; a press without movement is a click. */
  const dragNode = (node: Node) => (e: React.PointerEvent<SVGGElement>) => {
    e.stopPropagation();
    const svg = svgRef.current;
    const simulation = sim.current;
    if (!svg || !simulation) return;
    const start = { x: e.clientX, y: e.clientY };
    let moved = false;
    const toGraph = (cx: number, cy: number) => {
      const rect = svg.getBoundingClientRect();
      const v = view.current;
      return { x: (cx - rect.left - v.x) / v.k, y: (cy - rect.top - v.y) / v.k };
    };
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < 4) return;
      if (!moved) simulation.alphaTarget(0.25).restart();
      moved = true;
      const p = toGraph(ev.clientX, ev.clientY);
      node.fx = p.x;
      node.fy = p.y;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      simulation.alphaTarget(0);
      node.fx = null;
      node.fy = null;
      if (!moved) {
        if (node.kind === null) onGhost(node.title);
        else onSelect(node.id);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <div className="relative size-full">
      <svg
        ref={svgRef}
        role="img"
        aria-label={`Knowledge graph: ${graph.nodes.length} notes, ${graph.edges.length} links`}
        className="size-full cursor-grab touch-none select-none active:cursor-grabbing"
        style={{
          backgroundImage: "radial-gradient(#e4e7ec 1px, transparent 1px)",
          backgroundSize: "18px 18px",
        }}
      >
        <g ref={layerRef}>
          {links.map((l, i) => {
            const [s, t] = [idOf(l.source), idOf(l.target)];
            const dim = focus !== null && !(focus.has(s) && focus.has(t));
            return (
              <line
                key={`${s}-${t}-${i}`}
                data-link
                stroke={l.tentative ? "#d97706" : "#98a2b3"}
                strokeWidth={1.2}
                strokeDasharray={l.tentative ? "4 3" : undefined}
                opacity={dim ? 0.12 : 0.75}
              />
            );
          })}
          {nodes.map((n) => {
            const dim = focus !== null && !focus.has(n.id);
            const isSelected = n.id === selected;
            return (
              <g
                key={n.id}
                data-node={n.id}
                data-testid={`node-${n.title}`}
                role="button"
                tabIndex={0}
                aria-label={`${n.title}${n.suffix ? `, ${n.suffix}` : ""}`}
                aria-pressed={isSelected}
                className="cursor-pointer outline-none"
                opacity={dim ? 0.2 : 1}
                onPointerDown={dragNode(n)}
                onPointerEnter={() => setHovered(n.id)}
                onPointerLeave={() => setHovered((h) => (h === n.id ? null : h))}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    if (n.kind === null) onGhost(n.title);
                    else onSelect(n.id);
                  }
                }}
              >
                <rect
                  x={-n.w / 2}
                  y={-NODE_H / 2}
                  width={n.w}
                  height={NODE_H}
                  rx={n.look.pill ? NODE_H / 2 : 9}
                  fill={n.look.fill === "transparent" ? "#ffffff" : n.look.fill}
                  fillOpacity={n.look.fill === "transparent" ? 0.6 : 1}
                  stroke={isSelected ? "#4338ca" : n.look.stroke}
                  strokeWidth={isSelected ? 2.5 : 1.3}
                  strokeDasharray={n.look.dashed ? "4 3" : undefined}
                />
                <text
                  x={n.suffix ? -n.w / 2 + 14 : 0}
                  y={4.5}
                  textAnchor={n.suffix ? "start" : "middle"}
                  style={{ font: FONT }}
                  fill={n.look.text}
                  fontStyle={n.kind === null ? "italic" : undefined}
                >
                  {n.title}
                  {n.suffix && (
                    <tspan dx={6} style={{ font: SUFFIX_FONT }} fill={n.look.text} opacity={0.75}>
                      {n.suffix}
                    </tspan>
                  )}
                </text>
              </g>
            );
          })}
        </g>
      </svg>
      <div className="absolute top-3 right-3 flex gap-1.5">
        <button
          type="button"
          aria-label="Zoom out"
          onClick={() => zoomBy(0.8)}
          className="grid size-8 place-items-center rounded-md border border-input bg-card hover:bg-secondary"
        >
          <Minus className="size-4" />
        </button>
        <button
          type="button"
          aria-label="Zoom in"
          onClick={() => zoomBy(1.25)}
          className="grid size-8 place-items-center rounded-md border border-input bg-card hover:bg-secondary"
        >
          <Plus className="size-4" />
        </button>
        <button
          type="button"
          aria-label="Fit the graph"
          onClick={fit}
          className="grid size-8 place-items-center rounded-md border border-input bg-card hover:bg-secondary"
        >
          <Maximize className="size-4" />
        </button>
      </div>
    </div>
  );
});
