"use client";

import "react-grid-layout/css/styles.css";

import type { Visual } from "@lattice/shared-types";
import { useCallback, useMemo } from "react";
import ReactGridLayout, {
  useContainerWidth,
  verticalCompactor,
  type Layout,
} from "react-grid-layout";

import { useSheetEnv, useSheetState } from "./sheet-env";
import { VisualCard } from "./visual-card";

const ROW_HEIGHT = 52;
const GAP = 16;
/** Below this width visuals stack in one column, in grid order. */
const STACK_BELOW = 720;

function byPosition(a: Visual, b: Visual): number {
  return a.layout.y - b.layout.y || a.layout.x - b.layout.x;
}

/** The sheet's 12-column grid: drag by a visual's header and resize from its corner while
 * editing; each move or resize is one undoable command. */
export function SheetGrid() {
  const { store, canEdit } = useSheetEnv();
  const visuals = useSheetState((s) => s.doc.visuals);
  const editing = useSheetState((s) => s.editing) && canEdit;
  const { width, containerRef, mounted } = useContainerWidth();

  const layout = useMemo<Layout>(
    () =>
      visuals.map((v) => ({
        i: v.id,
        ...v.layout,
        minW: 2,
        minH: v.spec.kind === "kpi" ? 2 : 3,
      })),
    [visuals],
  );

  const commit = useCallback(
    (label: string) => (next: Layout) => {
      store.getState().edit(label, (doc) => {
        let changed = false;
        const visuals = doc.visuals.map((v) => {
          const item = next.find((l) => l.i === v.id);
          if (!item) return v;
          const placed = { x: item.x, y: item.y, w: item.w, h: item.h };
          if (JSON.stringify(placed) === JSON.stringify(v.layout)) return v;
          changed = true;
          return { ...v, layout: placed };
        });
        return changed ? { ...doc, visuals } : doc;
      });
    },
    [store],
  );

  if (visuals.length === 0) return null;
  return (
    <div
      ref={containerRef}
      className="min-w-0"
      data-testid="sheet-canvas"
      onClick={
        editing
          ? (e) => {
              // Clicking the canvas rather than a visual shows the sheet's settings.
              if (!(e.target as HTMLElement).closest("[data-visual]"))
                store.getState().selectVisual(null);
            }
          : undefined
      }
    >
      {mounted && width < STACK_BELOW ? (
        <div className="flex flex-col gap-4">
          {[...visuals].sort(byPosition).map((v) => (
            <div key={v.id} style={{ height: v.layout.h * ROW_HEIGHT + (v.layout.h - 1) * GAP }}>
              <VisualCard visual={v} />
            </div>
          ))}
        </div>
      ) : mounted ? (
        <ReactGridLayout
          width={width}
          layout={layout}
          gridConfig={{
            cols: 12,
            rowHeight: ROW_HEIGHT,
            margin: [GAP, GAP],
            containerPadding: [0, 0],
          }}
          dragConfig={{ enabled: editing, handle: ".visual-drag" }}
          resizeConfig={{ enabled: editing, handles: ["se"] }}
          compactor={verticalCompactor}
          onDragStop={commit("Move visual")}
          onResizeStop={commit("Resize visual")}
        >
          {visuals.map((v) => (
            <div key={v.id}>
              <VisualCard visual={v} />
            </div>
          ))}
        </ReactGridLayout>
      ) : null}
    </div>
  );
}
