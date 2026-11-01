import { expect, test, type Page } from "@playwright/test";

// Where the *gateway* reaches the bundled acme_ops database: `postgres:5432` inside Compose,
// `localhost:5433` when the gateway runs natively (make dev).
const PG_HOST = process.env.E2E_PG_HOST ?? "postgres";
const PG_PORT = process.env.E2E_PG_PORT ?? "5432";

async function signInDemo(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Work email").fill("demo@lattice.dev");
  await page.getByLabel("Password").fill("lattice-demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Projects", level: 1 })).toBeVisible();
}

async function openRevenueSources(page: Page) {
  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).click();
  await page.locator("nav").getByRole("link", { name: "Sources" }).click();
  await expect(page.getByRole("heading", { name: "Sources", level: 1 })).toBeVisible();
}

test("the demo dataset is profiled and explorable over the data socket", async ({ page }) => {
  await signInDemo(page);
  await openRevenueSources(page);
  const row = page.getByRole("row", { name: /Acme Retail orders \(demo\)/ });
  await expect(row.getByText("Healthy")).toBeVisible();
  await row.getByRole("link").click();

  await expect(
    page.getByRole("heading", { name: "Acme Retail orders (demo)", level: 1 }),
  ).toBeVisible();
  await page
    .getByRole("navigation", { name: "Tables" })
    .getByRole("button", { name: /orders/ })
    .click();
  await expect(page.getByText(/distinct/).first()).toBeVisible(); // column profiles
  const grid = page.getByRole("grid");
  await expect(grid.getByRole("columnheader", { name: "amount" })).toBeVisible();
  await expect(page.getByTestId("query-timings")).toContainText("100 rows");

  await page.getByLabel("Show").selectOption("summary");
  await page.getByLabel("Group by").selectOption("region");
  await page.getByLabel("Measure").selectOption("sum");
  await page.getByLabel("of").selectOption("amount");
  await expect(page.getByTestId("query-timings")).toContainText("3 rows");
  for (const region of ["AMER", "EMEA", "APAC"]) {
    await expect(grid.getByRole("gridcell", { name: region, exact: true })).toBeVisible();
  }
});

test("upload a CSV, then query it", async ({ page }) => {
  const email = `e2e-up-${Date.now()}@lattice.test`;
  await page.goto("/signup");
  await page.getByLabel("Your name").fill("Uma Upload");
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Workspace name").fill("Upload Co");
  await page.getByLabel("Password").fill("e2e-password-123");
  await page.getByRole("button", { name: "Create workspace" }).click();
  await expect(page).toHaveURL(/\/projects$/);

  await page.getByRole("button", { name: "+ New project" }).first().click();
  await page.getByRole("dialog").getByLabel("Name", { exact: true }).fill("Store regions");
  await page.getByRole("dialog").getByRole("button", { name: "Create project" }).click();
  await expect(page.getByRole("heading", { name: "Store regions", level: 1 })).toBeVisible();
  await page.locator("nav").getByRole("link", { name: "Sources" }).click();

  const csv = "store,region,revenue\nS1,EMEA,120.5\nS2,EMEA,80\nS3,AMER,300\nS4,APAC,42.25\n";
  await page.locator("#upload").setInputFiles({
    name: "store_regions.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(csv),
  });
  await expect(page.getByText("Loaded 1 table, 4 rows")).toBeVisible();
  await expect(
    page.getByRole("row", { name: /store_regions\.csv/ }).getByText("Healthy"),
  ).toBeVisible();

  await page.getByRole("link", { name: "Explore store_regions.csv →" }).click();
  await expect(page.getByTestId("query-timings")).toContainText("4 rows");
  await page.getByLabel("Show").selectOption("summary");
  await page.getByLabel("Group by").selectOption("region");
  await expect(page.getByTestId("query-timings")).toContainText("3 rows");
});

test("connect a live Postgres database and query it", async ({ page }) => {
  await signInDemo(page);
  await openRevenueSources(page);
  await page.getByRole("button", { name: /PG\s*PostgreSQL/ }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Host").fill(PG_HOST);
  await dialog.getByLabel("Port").fill(PG_PORT);
  await dialog.getByLabel("Database").fill("acme_ops");
  await dialog.getByLabel("User").fill("acme_reader");
  await dialog.getByLabel("Password").fill("acme-reader");
  await dialog.getByLabel("Display name").fill(`Ops ${Date.now()}`);
  await dialog.getByRole("button", { name: "Connect" }).click();

  await expect(page.getByText(/PostgreSQL · acme_reader@/)).toBeVisible();
  const tables = page.getByRole("navigation", { name: "Tables" });
  await expect(tables.getByRole("button", { name: /public\.refunds/ })).toBeVisible();
  await tables.getByRole("button", { name: /public\.stores/ }).click();
  await expect(page.getByTestId("query-timings")).toContainText("60 rows");
});

test("wrong database credentials are refused with a readable error", async ({ page }) => {
  await signInDemo(page);
  await openRevenueSources(page);
  await page.getByRole("button", { name: /MY\s*MySQL/ }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Host").fill("127.0.0.1");
  await dialog.getByLabel("Port").fill("1");
  await dialog.getByLabel("Database").fill("nope");
  await dialog.getByLabel("User").fill("nobody");
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Couldn't connect");
});
