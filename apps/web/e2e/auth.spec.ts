import { expect, test } from "@playwright/test";

const DEMO = { email: "demo@lattice.dev", password: "lattice-demo" };

async function signIn(page: import("@playwright/test").Page, email: string, password: string) {
  await page.goto("/login");
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
}

test("signed-out visitors are sent to sign in, then returned", async ({ page }) => {
  await page.goto("/settings");
  await expect(page).toHaveURL(/\/login\?next=%2Fsettings/);
  await page.getByLabel("Work email").fill(DEMO.email);
  await page.getByLabel("Password").fill(DEMO.password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(page.getByText(DEMO.email).first()).toBeVisible();
});

test("wrong password shows the gateway's message", async ({ page }) => {
  await signIn(page, DEMO.email, "definitely-wrong");
  // Scoped to the form: Next.js also renders a (hidden) route announcer with role="alert".
  await expect(page.locator("form").getByRole("alert")).toHaveText("invalid email or password");
  await expect(page).toHaveURL(/\/login/);
});

test("demo workspace: projects list, overview and members", async ({ page }) => {
  await signIn(page, DEMO.email, DEMO.password);
  await expect(page.getByRole("heading", { name: "Projects", level: 1 })).toBeVisible();
  const card = page.getByRole("link", { name: /Revenue Analytics.*sources/s });
  await expect(card).toBeVisible();
  await expect(card.getByText("Owner")).toBeVisible();

  await page.getByRole("tab", { name: "Shared with me" }).click();
  await expect(page).toHaveURL(/filter=shared/);
  await expect(page.getByText("Nothing shared yet")).toBeVisible();
  await page.getByRole("tab", { name: "All" }).click();

  await page.getByRole("link", { name: /Revenue Analytics.*sources/s }).click();
  await expect(page.getByRole("heading", { name: "Revenue Analytics", level: 1 })).toBeVisible();
  const members = page.locator("#members");
  await expect(members.getByText("Demo Admin")).toBeVisible();
  await expect(members.getByText("Priya R.")).toBeVisible();
  await expect(members.getByText("Owner")).toBeVisible();
});

test("create a workspace, then a project, then sign out", async ({ page }) => {
  const email = `e2e-${Date.now()}@lattice.test`;
  await page.goto("/signup");
  await page.getByLabel("Your name").fill("Erin Example");
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Workspace name").fill("E2E Retail");
  await page.getByLabel("Password").fill("e2e-password-123");
  await page.getByRole("button", { name: "Create workspace" }).click();

  await expect(page).toHaveURL(/\/projects$/);
  await expect(page.getByText("No projects yet")).toBeVisible();
  await expect(page.locator("nav").getByText("Admin · E2E Retail")).toBeVisible();

  await page.getByRole("button", { name: "+ New project" }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name", { exact: true }).fill("Pricing Experiments");
  await dialog
    .getByLabel("Description", { exact: true })
    .fill("A/B test readouts and price elasticity by category.");
  await dialog.getByRole("button", { name: "Create project" }).click();

  await expect(page.getByRole("heading", { name: "Pricing Experiments", level: 1 })).toBeVisible();
  await expect(page.locator("#members").getByText("Erin Example")).toBeVisible();

  await page.locator("nav").getByText("Erin Example").click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/login/);
  await page.goto("/projects");
  await expect(page).toHaveURL(/\/login/);
});
