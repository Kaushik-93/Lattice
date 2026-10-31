/**
 * Interaction-latency budgets (spec §3) that phase 0 can already measure. Results are also
 * written to perf-results/web.json for the phase report and CI regression tracking.
 *
 * - App shell + skeleton must paint fast (budget: skeletons within 100 ms of navigation
 *   commit; we gate FCP on a warm cache at 1.2 s, the sheet first-load budget).
 * - Data must replace skeletons quickly once the gateway answers.
 * - No main-thread task over 50 ms during load.
 */
import fs from "node:fs";
import path from "node:path";

import { expect, test, type Page } from "@playwright/test";

const results: Record<string, number> = {};

async function navTimings(page: Page) {
  return page.evaluate(() => {
    const nav = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming;
    const fcp = performance.getEntriesByName("first-contentful-paint")[0]?.startTime ?? NaN;
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

test("sign-in page: warm load within budget, no long tasks", async ({ page }) => {
  await trackLongTasks(page);
  await page.goto("/login"); // cold: fills the HTTP cache
  await page.reload({ waitUntil: "load" }); // warm
  const t = await navTimings(page);
  Object.assign(results, {
    login_ttfb_ms: t.ttfb,
    login_fcp_ms: t.fcp,
    login_dcl_ms: t.dcl,
    login_load_ms: t.load,
  });
  expect(t.fcp).toBeLessThan(1200);
  expect(t.dcl).toBeLessThan(1200);
  expect(Math.max(0, ...(await longTasks(page)))).toBeLessThan(50);
});

test("projects page: skeleton then data, within budget", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Work email").fill("demo@lattice.dev");
  await page.getByLabel("Password").fill("lattice-demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("link", { name: /Revenue Analytics.*sources/s })).toBeVisible();

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
  });
  expect(t.fcp).toBeLessThan(1200);
  expect(dataVisibleAt).toBeLessThan(1200);
  expect(Math.max(0, ...(await longTasks(page)))).toBeLessThan(50);

  // Client-side navigation to a project (prefetched on hover): the §3 "switch view" budget.
  const card = page.getByRole("link", { name: /Revenue Analytics.*sources/s });
  await card.hover();
  await page.waitForTimeout(300);
  const started = await page.evaluate(() => performance.now());
  await card.click();
  await page.locator("#members").getByText("Demo Admin").waitFor();
  const switched = (await page.evaluate(() => performance.now())) - started;
  results.project_switch_prefetched_ms = switched;
  expect(switched).toBeLessThan(300);
});

test.afterAll(() => {
  const dir = path.resolve(__dirname, "../../../perf-results");
  fs.mkdirSync(dir, { recursive: true });
  const rounded = Object.fromEntries(
    Object.entries(results).map(([k, v]) => [k, Math.round(v * 10) / 10]),
  );
  fs.writeFileSync(path.join(dir, "web.json"), JSON.stringify(rounded, null, 2) + "\n");
});
