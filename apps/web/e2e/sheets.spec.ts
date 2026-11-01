import { expect, test, type Locator, type Page } from "@playwright/test";

async function signInDemo(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Work email").fill("demo@lattice.dev");
  await page.getByLabel("Password").fill("lattice-demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Projects", level: 1 })).toBeVisible();
}

async function openSheets(page: Page) {
  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).click();
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("link", { name: "Sheets", exact: true })
    .click();
  await expect(page.getByTestId("sheet")).toBeVisible();
}

/** Waits for the sheet's next batch to finish rendering, after `before` batches. */
async function settled(page: Page, before = 0) {
  await expect
    .poll(async () => Number((await page.getByTestId("sheet").getAttribute("data-settled")) ?? 0), {
      timeout: 30_000,
    })
    .toBeGreaterThan(before);
}

/** HTML5 drag and drop with real pointer steps, measured after both ends are scrolled into view
 * (the drawer scrolls, so measuring first would aim at the wrong place). */
async function drag(page: Page, source: Locator, target: Locator) {
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const [a, b] = [await source.boundingBox(), await target.boundingBox()];
  if (!a || !b) throw new Error("drag ends aren't visible");
  await page.mouse.move(a.x + 20, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + 40, a.y + a.height / 2, { steps: 5 });
  await page.mouse.move(b.x + 40, b.y + 20, { steps: 10 });
  await page.mouse.up();
}

const settleCount = async (page: Page) =>
  Number((await page.getByTestId("sheet").getAttribute("data-settled")) ?? 0);

test("a sheet renders its visuals and cross-filters them", async ({ page }) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  await expect(page.getByRole("heading", { name: "Revenue Review", level: 1 })).toBeVisible();
  const revenue = page
    .getByRole("region", { name: "Net revenue", exact: true })
    .getByTestId("kpi-value");
  await expect(revenue).toHaveText(/^\$[\d.]+[KM]$/);
  const before = await revenue.innerText();

  // Picking EMEA on the region bars filters every other visual on the orders table.
  const n = await settleCount(page);
  const regions = page.getByRole("region", { name: "Net revenue by region" });
  await regions.getByRole("button", { name: /EMEA/ }).click();
  await settled(page, n);
  await expect(regions.getByRole("button", { name: /EMEA/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(revenue).not.toHaveText(before);

  // The header filter narrows the whole sheet too.
  const m = await settleCount(page);
  await page.getByRole("button", { name: /^Region:/ }).click();
  await page.getByRole("menuitemcheckbox", { name: "APAC" }).click();
  await page.keyboard.press("Escape");
  await settled(page, m);
  await expect(page.getByRole("button", { name: "Region: APAC" })).toBeVisible();
});

test("double-click drills down, the breadcrumb drills back up", async ({ page }) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  const regions = page.getByRole("region", { name: "Net revenue by region" });
  const n = await settleCount(page);
  await regions.getByRole("button", { name: /AMER/ }).dblclick();
  await settled(page, n);
  // Region → category: the bars now list categories within AMER.
  await expect(regions.getByRole("navigation", { name: "Drill path" })).toContainText("AMER");
  await expect(regions.getByRole("button", { name: /Outdoor/ })).toBeVisible();
  await regions
    .getByRole("navigation", { name: "Drill path" })
    .getByRole("button", { name: "All" })
    .click();
  await expect(regions.getByRole("button", { name: /EMEA/ })).toBeVisible();
});

test("edit a sheet: add a visual, undo, redo, and it's saved", async ({ page }) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  // A sheet of our own, so the demo sheets stay as seeded; removed even if the test fails.
  await page.getByRole("button", { name: "+ New sheet" }).click();
  await expect(page.getByRole("heading", { name: /^Sheet \d+$/, level: 1 })).toBeVisible();
  try {
    await page.getByRole("button", { name: "Add visual" }).first().click();
    await expect(page.getByRole("heading", { name: "Edit visual" })).toBeVisible();
    await page.getByLabel("Table").selectOption({ label: "Acme Retail orders (demo) · orders" });
    await page.getByLabel("Title").fill("Orders by status");
    await page.getByLabel("Group by", { exact: true }).selectOption("status");
    await page.getByRole("button", { name: "Donut", exact: true }).click();
    const visual = page.getByRole("region", { name: "Orders by status" });
    await expect(visual).toHaveAttribute("data-kind", "donut");
    await expect(visual.locator("canvas").first()).toBeVisible();

    // Undo the type change, the axis and the rename; then redo all three.
    await page.getByRole("button", { name: "Undo change visual type" }).click();
    await page.getByRole("button", { name: "Undo change axis" }).click();
    await page.getByRole("button", { name: "Undo rename visual" }).click();
    await expect(page.getByRole("region", { name: "Orders by status" })).toHaveCount(0);
    await page.getByRole("button", { name: "Redo rename visual" }).click();
    await page.getByRole("button", { name: "Redo change axis" }).click();
    await page.getByRole("button", { name: "Redo change visual type" }).click();
    await expect(visual).toHaveAttribute("data-kind", "donut");

    await expect(page.getByTestId("save-state")).toHaveText("Saved", { timeout: 10_000 });
    await page.reload();
    await expect(page.getByRole("region", { name: "Orders by status" })).toHaveAttribute(
      "data-kind",
      "donut",
    );
  } finally {
    await page.getByRole("button", { name: "Sheet actions" }).click();
    await page.getByRole("menuitem", { name: "Delete this sheet" }).click();
  }
  await expect(page.getByRole("heading", { name: "Revenue Review", level: 1 })).toBeVisible();
});

test("share a sheet by link and open it signed out", async ({ page, browser }) => {
  await signInDemo(page);
  await openSheets(page);
  await page.getByRole("tab", { name: /Regional ops/ }).click();
  await page.getByRole("link", { name: "Share sheet" }).click();
  await expect(page.getByRole("heading", { name: "Members & sharing" })).toBeVisible();
  await expect(page.getByRole("radio", { name: /Regional ops/ })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await page.getByRole("button", { name: /Anyone with the link/ }).click();
  const field = page.getByLabel("Share link");
  await expect(field).toHaveValue(/\/s\/[\w-]{20,}$/);
  const link = await field.inputValue();

  const anonymous = await browser.newContext();
  const viewer = await anonymous.newPage();
  await viewer.goto(link);
  await expect(viewer.getByTestId("shared-sheet")).toBeVisible();
  await expect(
    viewer
      .getByRole("button", { name: "Sign in" })
      .or(viewer.getByRole("link", { name: "Sign in" })),
  ).toBeVisible();
  await expect(
    viewer
      .getByRole("region", { name: "Revenue by category and region" })
      .locator("canvas")
      .first(),
  ).toBeVisible();
  await viewer.getByRole("radio", { name: "EMEA" }).click();
  await expect(viewer.getByRole("radio", { name: "EMEA" })).toHaveAttribute("aria-checked", "true");
  // Link viewers can't edit.
  await expect(viewer.getByRole("button", { name: "Edit" })).toHaveCount(0);

  // Turning the link off locks the viewer out.
  await page.getByRole("button", { name: "Turn off link" }).click();
  await expect(page.getByLabel("Share link")).toHaveCount(0);
  await viewer.reload();
  await expect(viewer.getByRole("heading", { name: "This link doesn't work" })).toBeVisible();
  await anonymous.close();
});

test("visuals chart governed metrics; raw columns are marked", async ({ page }) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  const net = page.getByRole("region", { name: "Net revenue", exact: true });
  const orders = page.getByRole("region", { name: "Orders", exact: true });
  // On a small KPI card the marker is an icon; its words are for screen readers.
  await expect(orders.getByText("Not governed")).toBeAttached();
  await expect(net.getByText("Not governed")).toHaveCount(0);
  await expect(net.getByText("Unapproved logic")).toHaveCount(0);

  // The editor offers metrics first.
  await page.getByRole("button", { name: "Net revenue by region: more actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  const picker = page.getByRole("complementary", { name: "Editor" }).getByLabel("Metric");
  await expect(picker.locator("option:checked")).toHaveText("Net revenue");
  await page.getByRole("button", { name: "Done editing" }).click();

  // A draft metric is usable, flagged on the visual. (On a sheet of our own.)
  await page.getByRole("button", { name: "+ New sheet" }).click();
  await expect(page.getByRole("heading", { name: /^Sheet \d+$/, level: 1 })).toBeVisible();
  try {
    await page.getByRole("button", { name: "Add visual" }).first().click();
    await page.getByLabel("Table").selectOption({ label: "Acme Retail orders (demo) · orders" });
    await page.getByLabel("Title").fill("Refunds");
    await page.getByLabel("Metric").selectOption({ label: "Refund rate · not approved" });
    await expect(
      page.getByRole("region", { name: "Refunds" }).getByText("Unapproved logic"),
    ).toBeVisible({ timeout: 15_000 });
  } finally {
    await page.getByRole("button", { name: "Sheet actions" }).click();
    await page.getByRole("menuitem", { name: "Delete this sheet" }).click();
  }
});

test("fiscal periods, comparisons and ranges to date", async ({ page }) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  await page.getByRole("button", { name: "+ New sheet" }).click();
  await expect(page.getByRole("heading", { name: /^Sheet \d+$/, level: 1 })).toBeVisible();
  try {
    // Bars by fiscal year: the calendar starts in February, and years are named after the
    // year they end in (FY2027 runs Feb 2026–Jan 2027).
    await page.getByRole("button", { name: "Add visual" }).first().click();
    await page.getByLabel("Table").selectOption({ label: "Acme Retail orders (demo) · orders" });
    await page.getByLabel("Title").fill("By fiscal year");
    await page.getByRole("button", { name: "Bar", exact: true }).click();
    await page.getByLabel("Group by", { exact: true }).selectOption("order_date");
    await page.getByLabel("Group by grain").selectOption("fiscal_year");
    const bars = page.getByRole("region", { name: "By fiscal year" });
    for (const year of ["FY2025", "FY2026", "FY2027"]) {
      await expect(bars.getByText(year, { exact: true })).toBeVisible({ timeout: 15_000 });
    }

    // A KPI against the same period last year, over fiscal year to date.
    await page.getByRole("button", { name: "Add visual" }).first().click();
    await page.getByLabel("Table").selectOption({ label: "Acme Retail orders (demo) · orders" });
    await page.getByLabel("Title").fill("Revenue YoY");
    await page.getByRole("button", { name: "KPI", exact: true }).click();
    await page.getByLabel("Compare over the sheet's date range on").selectOption("order_date");
    await page.getByLabel("Compared with").selectOption("previous_year");

    // A date filter defaulting to fiscal year to date.
    await page.getByRole("button", { name: "Sheet settings" }).click();
    await page.getByLabel("Filter on column").selectOption("order_date");
    await page.getByRole("button", { name: "Add", exact: true }).click();
    await page
      .getByLabel("Default range of the order_date filter")
      .selectOption("fiscal_year_to_date");
    const kpi = page.getByRole("region", { name: "Revenue YoY" });
    await expect(kpi).toContainText("vs same period last year", { timeout: 15_000 });
    await page.getByRole("button", { name: /order_date range/ }).click();
    await expect(page.getByRole("button", { name: "Fiscal quarter to date" })).toBeVisible();
    await page.keyboard.press("Escape");
  } finally {
    await page.getByRole("button", { name: "Sheet actions" }).click();
    await page.getByRole("menuitem", { name: "Delete this sheet" }).click();
  }
});

test("edit mode stays on across sheets; the drawer edits a visual by tab and by dragging fields", async ({
  page,
}) => {
  await signInDemo(page);
  await openSheets(page);
  await settled(page);
  await page.getByRole("button", { name: "Edit mode" }).click();
  const done = page.getByRole("button", { name: "Done", exact: true });
  await expect(done).toBeVisible();
  // Global: still on in another sheet, and after a reload.
  await page.getByRole("tab", { name: /Regional ops/ }).click();
  await expect(page.getByRole("heading", { name: "Regional ops", level: 1 })).toBeVisible();
  await expect(done).toBeVisible();
  await page.reload();
  await expect(done).toBeVisible();

  await page.getByRole("button", { name: "+ New sheet" }).click();
  await expect(page.getByRole("heading", { name: /^Sheet \d+$/, level: 1 })).toBeVisible();
  try {
    await page.getByRole("button", { name: "Add visual" }).first().click();
    const editor = page.getByRole("complementary", { name: "Editor" });
    await editor.getByLabel("Table").selectOption({ label: "Acme Retail orders (demo) · orders" });
    await editor.getByLabel("Title").fill("Dragged");
    await editor.getByRole("button", { name: "Column", exact: true }).click();

    // Drag a metric onto the values, and a column onto the axis.
    const fields = editor.getByRole("region", { name: "Fields" });
    await drag(
      page,
      fields.locator('[data-field="Net revenue"]'),
      editor.locator('[data-slot="values"]'),
    );
    await expect(editor.getByLabel("Metric").locator("option:checked")).toHaveText("Net revenue");
    await drag(
      page,
      fields.locator('[data-field="category"]'),
      editor.locator('[data-slot="axis"]'),
    );
    await expect(editor.getByLabel("Group by", { exact: true })).toHaveValue("category");
    // The same from the keyboard: a field's button adds it to a slot.
    await fields.getByLabel("Add channel to…").click();
    await page.getByRole("menuitem", { name: "Split by" }).click();
    await expect(editor.getByLabel("Split by", { exact: true })).toHaveValue("channel");
    await expect(
      page.getByRole("region", { name: "Dragged" }).locator("canvas").first(),
    ).toBeVisible();

    // Tabs: formatting and filters live on their own.
    await editor.getByRole("tab", { name: "Format" }).click();
    await expect(editor.getByLabel("Number format")).toBeVisible();
    await editor.getByRole("tab", { name: "Filters" }).click();
    await expect(editor.getByLabel("Sort")).toBeVisible();
    // Clicking the canvas shows the sheet's settings.
    const canvas = page.getByTestId("sheet-canvas");
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: (box?.width ?? 800) - 10, y: 10 } });
    await expect(editor.getByRole("heading", { name: "Sheet", exact: true })).toBeVisible();
  } finally {
    await page.getByRole("button", { name: "Sheet actions" }).click();
    await page.getByRole("menuitem", { name: "Delete this sheet" }).click();
  }
});
