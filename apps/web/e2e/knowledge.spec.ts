import { expect, test, type Page } from "@playwright/test";

async function signInDemo(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Work email").fill("demo@lattice.dev");
  await page.getByLabel("Password").fill("lattice-demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Projects", level: 1 })).toBeVisible();
}

async function openLogic(page: Page) {
  await page.getByRole("link", { name: /Business logic/ }).click();
  await expect(page.getByRole("heading", { name: "Business logic", level: 1 })).toBeVisible();
  await expect(page.getByTestId("node-Net revenue")).toBeVisible();
}

test("the knowledge graph links notes and shows what's waiting for review", async ({ page }) => {
  await signInDemo(page);
  // The sidebar counts what's waiting for the viewer's review.
  await expect(page.getByRole("link", { name: /Business logic/ })).toContainText(/\d/);
  await openLogic(page);

  await page.getByTestId("node-Net revenue").click();
  const panel = page.getByTestId("note-panel");
  await expect(panel.getByRole("heading", { name: "Net revenue" })).toBeVisible();
  await expect(panel).toContainText("Approved");
  await expect(panel).toContainText("SUM(order.amount)");
  // Following a [[link]] opens the linked note.
  await panel.getByRole("button", { name: "Fiscal calendar" }).first().click();
  await expect(panel.getByRole("heading", { name: "Fiscal calendar" })).toBeVisible();

  // A pending change shows its diff against the live version.
  const change = page
    .getByTestId("queue-item")
    .filter({ hasText: "Change to the metric “Active customer”" });
  await change.getByRole("button", { name: "Edit first" }).click();
  await expect(page.getByTestId("note-diff")).toContainText("60 days");
  await expect(page.getByTestId("note-diff")).toContainText("90 days");
});

test("a note goes from draft to approved, and a change can be turned down", async ({ page }) => {
  await signInDemo(page);
  await openLogic(page);
  const title = `Gross margin ${Date.now()}`;

  await page.getByRole("button", { name: "+ New note" }).click();
  const editor = page.getByTestId("note-editor");
  await editor.getByLabel("Title").fill(title);
  await editor.getByLabel("Owning team").fill("Finance");
  // Typing [[ suggests notes to link.
  const body = editor.getByLabel("Definition in plain English");
  await body.fill("Revenue left after costs, per [[Ord");
  await page.getByRole("option", { name: "Order", exact: true }).click();
  await expect(body).toHaveValue("Revenue left after costs, per [[Order]]");
  await editor.getByLabel("Formula").fill("SUM(order.amount) - SUM(order.cost)");
  await editor.getByRole("button", { name: "Save draft" }).click();

  const panel = page.getByTestId("note-panel");
  await expect(panel.getByRole("heading", { name: title })).toBeVisible();
  await expect(panel).toContainText("Draft");
  await expect(page.getByTestId(`node-${title}`)).toBeVisible();

  await editor.getByRole("button", { name: "Submit for review" }).click();
  await expect(panel).toContainText("Needs review");
  await panel.getByRole("button", { name: "Approve" }).click();
  await expect(panel).toContainText("Approved");

  // Propose a change, then turn it down with a reason; version 1 stays live.
  await editor
    .getByLabel("Definition in plain English")
    .fill("Revenue left after costs and shipping, per [[Order]]");
  await editor.getByLabel("Why this change?").fill("Include shipping");
  await editor.getByRole("button", { name: "Submit for review" }).click();
  await expect(panel).toContainText("change in review");
  await panel.getByRole("button", { name: "Not right" }).click();
  await panel.getByLabel("What's not right?").fill("Shipping is a separate metric.");
  await panel.getByRole("button", { name: "Reject" }).click();
  await expect(panel).toContainText("Approved");
  await editor.getByRole("tab", { name: "Version history" }).click();
  await expect(editor).toContainText("Rejected: Shipping is a separate metric.");

  // Retire it so the demo graph stays as seeded.
  await page.request.post(
    `${process.env.GATEWAY_URL ?? "http://localhost:8080"}/api/knowledge/notes/${await noteId(page)}/deprecate`,
    { data: { deprecated: true }, headers: { origin: "http://localhost:3000" } },
  );
});

test("a definition can't be approved while an example case fails", async ({ page }) => {
  await signInDemo(page);
  await openLogic(page);
  const title = `Settled revenue ${Date.now()}`;
  await page.getByRole("button", { name: "+ New note" }).click();
  const editor = page.getByTestId("note-editor");
  await editor.getByLabel("Title").fill(title);
  await editor.getByLabel("Formula").fill("SUM(order.amount)");
  await editor.getByRole("button", { name: "Start from an example" }).click();
  await expect(editor.getByLabel("Example cases")).toHaveValue(/Refunds don't count/);
  await editor.getByRole("button", { name: "Submit for review" }).click();

  const panel = page.getByTestId("note-panel");
  await expect(panel.getByRole("heading", { name: title })).toBeVisible();
  const cases = panel.getByRole("region", { name: "Example cases" });
  await expect(cases).toContainText("Expected 100, got 140.");
  await expect(panel.getByRole("button", { name: "Approve" })).toBeDisabled();
  await expect(panel.getByRole("region", { name: "Impact" })).toContainText(
    "Nothing else uses it yet.",
  );

  // Fixed, it passes and can be approved.
  await editor.getByLabel("Formula").fill("SUM(order.amount) WHERE [[Settled orders only]]");
  await editor.getByRole("button", { name: "Submit for review" }).click();
  await expect(cases).toContainText("Gives 100, as expected.");
  await panel.getByRole("button", { name: "Approve" }).click();
  await expect(panel).toContainText("Approved");

  await page.request.post(
    `${process.env.GATEWAY_URL ?? "http://localhost:8080"}/api/knowledge/notes/${await noteId(page)}/deprecate`,
    { data: { deprecated: true }, headers: { origin: "http://localhost:3000" } },
  );
});

test("a link to an unwritten note offers to create it", async ({ page }) => {
  await signInDemo(page);
  await openLogic(page);
  await page.getByTestId("node-Revenue Review").click();
  await expect(page.getByTestId("note-editor").getByLabel("Title")).toHaveValue("Revenue Review");
});

async function noteId(page: Page): Promise<string> {
  return new URL(page.url()).searchParams.get("note") ?? "";
}

test("profiles of connected tables suggest knowledge, with their evidence", async ({ page }) => {
  await signInDemo(page);
  // Profile the demo's orders, as a person would after connecting it.
  const gateway = process.env.GATEWAY_URL ?? "http://localhost:8080";
  const headers = { origin: "http://localhost:3000" };
  const projects = await (await page.request.get(`${gateway}/api/projects`, { headers })).json();
  const project = projects.projects.find((p: { name: string }) => p.name === "Revenue Analytics");
  const model = await (
    await page.request.get(`${gateway}/api/projects/${project.id}/model`, { headers })
  ).json();
  const orders = model.entities.find((e: { title: string }) => e.title === "Order").table.table_id;
  await page.request.post(`${gateway}/api/projects/${project.id}/tables/${orders}/profile`, {
    headers,
  });

  await openLogic(page);
  const section = page.getByRole("region", { name: /Picked up from your data/ });
  await expect(section).toBeVisible();
  const channel = section.getByRole("listitem", { name: "Dimension: Channel" });
  await expect(channel).toContainText(/values, e\.g\./);
  await expect(channel.getByRole("button", { name: "Accept" })).toBeVisible();
  // What the graph already has isn't suggested again.
  await expect(section.getByRole("listitem", { name: "Dimension: Region" })).toHaveCount(0);
});
