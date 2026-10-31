import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { safeNextPath } from "./form-bits";
import { LoginForm } from "./login-form";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace, push: vi.fn() }) }));

type Handler = (url: string, init?: RequestInit) => Response;

function mockGateway(login: Handler) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/api/auth/providers")) {
        return Response.json({
          providers: [{ id: "google", label: "Continue with Google" }],
          password: true,
        });
      }
      if (url.endsWith("/api/auth/login")) return login(url, init);
      return new Response(null, { status: 404 });
    }),
  );
}

function renderForm() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <LoginForm />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => replace.mockReset());
afterEach(() => vi.unstubAllGlobals());

describe("LoginForm", () => {
  it("signs in and remembers the session by default", async () => {
    let sent: unknown;
    mockGateway((_url, init) => {
      sent = JSON.parse(String(init?.body));
      return Response.json({ user: { id: "u1" } });
    });
    renderForm();

    await userEvent.type(screen.getByLabelText("Work email"), "ana@acme.example");
    await userEvent.type(screen.getByLabelText("Password"), "correct-horse-battery");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => expect(replace).toHaveBeenCalledWith("/projects"));
    expect(sent).toEqual({
      email: "ana@acme.example",
      password: "correct-horse-battery",
      remember: true,
    });
  });

  it("shows the gateway's error message on bad credentials", async () => {
    mockGateway(() =>
      Response.json(
        { error: { code: "invalid_credentials", message: "invalid email or password" } },
        { status: 401 },
      ),
    );
    renderForm();

    await userEvent.type(screen.getByLabelText("Work email"), "ana@acme.example");
    await userEvent.type(screen.getByLabelText("Password"), "wrong-password");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("invalid email or password");
    expect(replace).not.toHaveBeenCalled();
  });

  it("enables only the identity providers the gateway has configured", async () => {
    mockGateway(() => new Response(null, { status: 500 }));
    renderForm();

    const google = await screen.findByRole("link", { name: "Continue with Google" });
    expect(google).toHaveAttribute(
      "href",
      expect.stringContaining("/api/auth/oidc/google/start?remember=true"),
    );
    expect(screen.getByRole("button", { name: "Continue with Microsoft" })).toBeDisabled();
  });
});

describe("safeNextPath", () => {
  it("allows same-origin paths only", () => {
    expect(safeNextPath("/projects/abc")).toBe("/projects/abc");
    expect(safeNextPath("//evil.example")).toBe("/projects");
    expect(safeNextPath("/\\evil.example")).toBe("/projects");
    expect(safeNextPath("https://evil.example")).toBe("/projects");
    expect(safeNextPath(null)).toBe("/projects");
  });
});
