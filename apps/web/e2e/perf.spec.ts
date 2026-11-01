/**
 * Interaction-latency budgets (spec §3) that phase 0 can already measure. Results are also
 * written to perf-results/web.json for the phase report and CI regression tracking.
 *
 * - App shell + skeleton must paint fast (FCP is reported against the 100 ms skeleton budget and
 *   gated at 1.2 s, the first-load budget).
 * - Data must replace skeletons within the 1.2 s load budget.
 * - No main-thread task over 50 ms while interacting.
 */
import fs from "node:fs";
import path from "node:path";

import { expect, test, type Page } from "@playwright/test";

const results: Record<string, number> = {};

async function navTimings(page: Page) {
  return page.evaluate(async () => {
    const nav = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming;
    // Headless Chromium on Linux can record the paint entry after `load`; wait for it (buffered)
    // instead of reading once, which yields NaN on CI.
    const fcp = await new Promise<number>((resolve) => {
      const timeout = setTimeout(() => resolve(NaN), 5000);
      new PerformanceObserver((list, observer) => {
        const entry = list.getEntriesByName("first-contentful-paint")[0];
        if (entry) {
          clearTimeout(timeout);
          observer.disconnect();
          resolve(entry.startTime);
        }
      }).observe({ type: "paint", buffered: true });
    });
    return {
      ttfb: nav.responseStart,
      fcp,
      domInteractive: nav.domInteractive,
      dcl: nav.domContentLoadedEventEnd,
      load: nav.loadEventEnd,
    };
  });
}

async function trackLongTasks(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __longTasks: number[] };
    w.__longTasks = [];
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) w.__longTasks.push(entry.duration);
      }).observe({ type: "longtask", buffered: true });
    } catch {
      /* unsupported */
    }
  });
}

const longTasks = (page: Page) =>
  page.evaluate(() => (window as unknown as { __longTasks: number[] }).__longTasks);

async function signInDemo(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Work email").fill("demo@lattice.dev");
  await page.getByLabel("Password").fill("lattice-demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("link", { name: /Revenue Analytics.*sources/s })).toBeVisible();
}

const longest = (tasks: number[]) => Math.max(0, ...tasks);

// Page loads are gated on the §3 load budget (< 1.2 s to interactive). Script evaluation and
// hydration may exceed 50 ms on slow CPUs; that is reported, not gated — §3's 50 ms task budget
// is for interactions, which the last test gates.

test("sign-in page: warm load within budget", async ({ page }) => {
  await trackLongTasks(page);
  await page.goto("/login"); // cold: fills the HTTP cache
  await page.reload({ waitUntil: "load" }); // warm
  const t = await navTimings(page);
  Object.assign(results, {
    login_ttfb_ms: t.ttfb,
    login_fcp_ms: t.fcp,
    login_dcl_ms: t.dcl,
    login_load_ms: t.load,
    login_longest_task_ms: longest(await longTasks(page)),
  });
  expect(t.fcp).toBeLessThan(1200);
  expect(t.dcl).toBeLessThan(1200);
});

test("projects page: skeleton then data, within budget", async ({ page }) => {
  await signInDemo(page);
  await trackLongTasks(page);
  await page.reload({ waitUntil: "commit" });
  // The prerendered shell paints before any gateway response arrives.
  await page.getByRole("heading", { name: "Projects", level: 1 }).waitFor();
  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).waitFor();
  const dataVisibleAt = await page.evaluate(() => performance.now());
  await page.waitForLoadState("load");
  const t = await navTimings(page);

  Object.assign(results, {
    projects_ttfb_ms: t.ttfb,
    projects_fcp_ms: t.fcp,
    projects_dcl_ms: t.dcl,
    projects_data_visible_ms: dataVisibleAt,
    projects_longest_task_ms: longest(await longTasks(page)),
  });
  expect(t.fcp).toBeLessThan(1200);
  expect(dataVisibleAt).toBeLessThan(1200);
});

test("interactions: no main-thread task over 50 ms; prefetched project switch", async ({
  page,
}) => {
  await signInDemo(page);
  await page.waitForLoadState("load");
  await page.waitForTimeout(500); // let load-time work settle; measure interactions only
  await page.evaluate(() => {
    (window as unknown as { __longTasks: number[] }).__longTasks = [];
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries())
          (window as unknown as { __longTasks: number[] }).__longTasks.push(entry.duration);
      }).observe({ type: "longtask" });
    } catch {
      /* unsupported */
    }
  });

  const switches: number[] = [];
  for (const name of ["Owned by me", "Shared with me", "All"]) {
    const tab = page.getByRole("tab", { name });
    const started = await page.evaluate(() => performance.now());
    await tab.click();
    await expect(tab).toHaveAttribute("aria-selected", "true");
    switches.push((await page.evaluate(() => performance.now())) - started);
  }

  const card = page.getByRole("link", { name: /Revenue Analytics.*sources/s });
  await card.hover();
  await page.waitForTimeout(300);
  const started = await page.evaluate(() => performance.now());
  await card.click();
  await page.locator("#members").getByText("Demo Admin").waitFor();
  const switched = (await page.evaluate(() => performance.now())) - started;

  const interactionTasks = await longTasks(page);
  Object.assign(results, {
    filter_switch_max_ms: Math.max(...switches),
    project_switch_prefetched_ms: switched,
    interaction_longest_task_ms: longest(interactionTasks),
  });
  expect(longest(interactionTasks)).toBeLessThan(50);
  // No §3 budget for project switches (sheets get < 100 ms in phase 2); this guards regressions.
  expect(switched).toBeLessThan(500);
});

test("data socket: regrouping the 1M-row demo table re-renders within the cross-filter budget", async ({
  page,
}) => {
  await signInDemo(page);
  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).click();
  await page.locator("nav").getByRole("link", { name: "Sources" }).click();
  await page
    .getByRole("row", { name: /Acme Retail orders/ })
    .getByRole("link")
    .click();
  await page
    .getByRole("navigation", { name: "Tables" })
    .getByRole("button", { name: /orders/ })
    .click();
  await expect(page.getByTestId("query-timings")).toContainText("100 rows");
  await page.getByLabel("Show").selectOption("summary");
  await page.getByLabel("Measure").selectOption("sum");
  await page.getByLabel("of").selectOption("amount");

  // Each change sends a new batch (superseding the last) and renders the Arrow result.
  const steps: [string, string][] = [
    ["region", "3 rows"],
    ["category", "8 rows"],
    ["channel", "2 rows"],
    ["status", "3 rows"],
    ["store_id", "60 rows"],
  ];
  const latencies: number[] = [];
  const gateway: number[] = [];
  for (const [column, rows] of steps) {
    const started = await page.evaluate(() => performance.now());
    await page.getByLabel("Group by").selectOption(column);
    await expect(page.getByTestId("query-timings")).toContainText(rows);
    await expect(page.getByTestId("query-timings")).not.toHaveClass(/opacity-50/);
    latencies.push((await page.evaluate(() => performance.now())) - started);
    const text = await page.getByTestId("query-timings").innerText();
    gateway.push(Number(/gateway\s+([\d.]+) ms/.exec(text)?.[1] ?? NaN));
  }
  latencies.sort((a, b) => a - b);
  gateway.sort((a, b) => a - b);
  const median = latencies[Math.floor(latencies.length / 2)] ?? NaN;
  Object.assign(results, {
    regroup_1m_rows_median_ms: median,
    regroup_1m_rows_max_ms: latencies.at(-1) ?? NaN,
    regroup_1m_rows_gateway_median_ms: gateway[Math.floor(gateway.length / 2)] ?? NaN,
  });
  // §3: cross-filter → visuals updated < 150 ms including network.
  expect(median).toBeLessThan(150);
});

async function openSheet(page: Page, name: string) {
  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).click();
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("link", { name, exact: true })
    .click();
  await expect(page.getByRole("heading", { name, level: 1 })).toBeVisible();
}

const sheetAttr = async (page: Page, attr: string) =>
  Number((await page.getByTestId("sheet").getAttribute(attr)) ?? NaN);

async function nextSettle(page: Page, before: number): Promise<number> {
  await expect
    .poll(() => sheetAttr(page, "data-settled"), { timeout: 30_000 })
    .toBeGreaterThan(before);
  return sheetAttr(page, "data-settle-ms");
}

const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)] ?? NaN;

test("sheet: a cross-filter click updates every visual within budget", async ({ page }) => {
  await trackLongTasks(page);
  await signInDemo(page);
  await openSheet(page, "Revenue Review");
  await nextSettle(page, 0);
  const regions = page.getByRole("region", { name: "Net revenue by region" });
  const times: number[] = [];
  const tasksBefore = (await longTasks(page)).length;
  // Each click re-runs the sheet's 7 visuals as one batch on 1M rows; the timing is measured
  // in the page, from the click's state change to the frame after the last result rendered.
  for (const region of ["EMEA", "APAC", "AMER", "EMEA", "APAC", "AMER", "AMER"]) {
    const n = await sheetAttr(page, "data-settled");
    await regions.getByRole("button", { name: new RegExp(region) }).click();
    times.push(await nextSettle(page, n));
  }
  const tasks = (await longTasks(page)).slice(tasksBefore);
  Object.assign(results, {
    sheet_cross_filter_median_ms: median(times),
    sheet_cross_filter_max_ms: Math.max(...times),
    sheet_interaction_longest_task_ms: longest(tasks),
  });
  // §3: cross-filter → all visuals updated < 150 ms including network; no task over 50 ms.
  expect(median(times)).toBeLessThan(150);
  expect(longest(tasks)).toBeLessThan(50);
});

test("sheet: switching to a prefetched sheet is under 100 ms", async ({ page }) => {
  await signInDemo(page);
  await openSheet(page, "Revenue Review");
  await nextSettle(page, 0);
  // Neighbours are prefetched after the first sheet settles; give that a moment.
  await page.waitForTimeout(1500);
  const switches: number[] = [];
  for (const name of ["Regional ops", "Revenue Review", "Regional ops"]) {
    await page.getByRole("tab", { name: new RegExp(name) }).click();
    await expect(page.getByRole("heading", { name, level: 1 })).toBeVisible();
    await expect
      .poll(() => sheetAttr(page, "data-switch-ms"), { timeout: 30_000 })
      .toBeGreaterThan(0);
    switches.push(await sheetAttr(page, "data-switch-ms"));
  }
  Object.assign(results, { sheet_switch_prefetched_median_ms: median(switches) });
  expect(median(switches)).toBeLessThan(100);
});

test("scatter: pan and zoom over a million points stay at 60 fps", async ({ page }) => {
  await trackLongTasks(page);
  await signInDemo(page);
  await openSheet(page, "Revenue Review");
  await page.getByRole("tab", { name: /Order explorer/ }).click();
  const plot = page.getByTestId("scatter-gl");
  await expect(plot).toHaveAttribute("data-points", "1000000", { timeout: 60_000 });
  await expect(plot).toHaveAttribute("data-frame-at", /\d/);
  const renderer = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    return info ? String(gl?.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "unknown";
  });
  const before = (await longTasks(page)).length;
  const box = (await plot.boundingBox())!;
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  // Sample frame intervals while zooming in and out and panning across the plot.
  await page.evaluate(() => {
    const w = window as unknown as { __frames: number[]; __sampling: boolean };
    w.__frames = [];
    w.__sampling = true;
    let last = performance.now();
    const tick = (t: number) => {
      w.__frames.push(t - last);
      last = t;
      if (w.__sampling) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.mouse.move(cx, cy);
  for (let i = 0; i < 30; i++) await page.mouse.wheel(0, i < 15 ? -120 : 120);
  await page.mouse.down();
  await page.mouse.move(cx - 300, cy + 40, { steps: 40 });
  await page.mouse.move(cx + 200, cy - 40, { steps: 40 });
  await page.mouse.up();
  const frames = await page.evaluate(() => {
    const w = window as unknown as { __frames: number[]; __sampling: boolean };
    w.__sampling = false;
    return w.__frames.slice(2);
  });
  // p95 frame time is the honest measure: a slow render stretches the frames it lands in.
  const sorted = [...frames].sort((a, b) => a - b);
  const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? NaN;
  const fps = 1000 / median(frames);
  const tasks = (await longTasks(page)).slice(before);
  Object.assign(results, {
    scatter_1m_points_median_fps: fps,
    scatter_1m_points_p95_frame_ms: p95,
    scatter_interaction_longest_task_ms: longest(tasks),
  });
  test.info().annotations.push({ type: "webgl", description: renderer });
  // Software GL (SwiftShader on GPU-less CI runners) draws a million points per frame on the
  // CPU; the 60 fps budget is for real GPUs, so it's reported, not gated, there.
  test.skip(
    /SwiftShader|llvmpipe/i.test(renderer),
    `software WebGL (${renderer}): p95 frame ${p95.toFixed(1)} ms`,
  );
  expect(longest(tasks)).toBeLessThan(50);
  // 60 fps with a little headroom for frames that land between input events.
  expect(p95).toBeLessThanOrEqual(20);
});

test.afterAll(() => {
  const dir = path.resolve(__dirname, "../../../perf-results");
  fs.mkdirSync(dir, { recursive: true });
  const rounded = Object.fromEntries(
    Object.entries(results).map(([k, v]) => [k, Math.round(v * 10) / 10]),
  );
  fs.writeFileSync(path.join(dir, "web.json"), JSON.stringify(rounded, null, 2) + "\n");
});
