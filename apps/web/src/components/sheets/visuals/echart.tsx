"use client";

import type { EChartsOption } from "echarts";
import { BarChart, HeatmapChart, LineChart, PieChart } from "echarts/charts";
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
} from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { memo, useEffect, useRef } from "react";

// Only what the sheet's chart types use, so the bundle carries no unused chart code.
echarts.use([
  BarChart,
  HeatmapChart,
  LineChart,
  PieChart,
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
  CanvasRenderer,
]);

export interface Pick {
  dataIndex: number;
  seriesIndex: number;
  /** Heatmap cells carry [x, y, value]. */
  value: unknown;
}

interface Props {
  option: EChartsOption;
  label: string;
  onPick?: (pick: Pick) => void;
  onDoublePick?: (pick: Pick) => void;
  /** Called with the zoomed window (0–100 %) when the user zooms a category axis. */
  onZoom?: (start: number, end: number) => void;
}

/** A canvas-rendered ECharts instance that follows its container's size. */
export const EChart = memo(function EChart({ option, label, onPick, onDoublePick, onZoom }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const handlers = useRef({ onPick, onDoublePick, onZoom });
  handlers.current = { onPick, onDoublePick, onZoom };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const instance = echarts.init(el, null, { renderer: "canvas" });
    chart.current = instance;
    const pick = (params: { dataIndex: number; seriesIndex?: number; value: unknown }): Pick => ({
      dataIndex: params.dataIndex,
      seriesIndex: params.seriesIndex ?? 0,
      value: params.value,
    });
    instance.on("click", (p) => handlers.current.onPick?.(pick(p as Parameters<typeof pick>[0])));
    instance.on("dblclick", (p) =>
      handlers.current.onDoublePick?.(pick(p as Parameters<typeof pick>[0])),
    );
    instance.on("datazoom", () => {
      const zoom = (
        instance.getOption().dataZoom as { start?: number; end?: number }[] | undefined
      )?.[0];
      if (zoom) handlers.current.onZoom?.(zoom.start ?? 0, zoom.end ?? 100);
    });
    const observer = new ResizeObserver(() => instance.resize());
    observer.observe(el);
    return () => {
      observer.disconnect();
      instance.dispose();
      chart.current = null;
    };
  }, []);

  useEffect(() => {
    chart.current?.setOption(option, { notMerge: true, lazyUpdate: true });
  }, [option]);

  return <div ref={ref} role="img" aria-label={label} className="size-full min-h-0" />;
});

/** The chart as a PNG data URL (for export). */
export function chartImage(container: HTMLElement): string | null {
  const el = container.querySelector<HTMLDivElement>("[_echarts_instance_]");
  const instance = el ? echarts.getInstanceByDom(el) : undefined;
  return instance?.getDataURL({ type: "png", pixelRatio: 2, backgroundColor: "#ffffff" }) ?? null;
}
