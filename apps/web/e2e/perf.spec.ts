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

test.afterAll(() => {
  const dir = path.resolve(__dirname, "../../../perf-results");
  fs.mkdirSync(dir, { recursive: true });
  const rounded = Object.fromEntries(
    Object.entries(results).map(([k, v]) => [k, Math.round(v * 10) / 10]),
  );
  fs.writeFileSync(path.join(dir, "web.json"), JSON.stringify(rounded, null, 2) + "\n");
});
