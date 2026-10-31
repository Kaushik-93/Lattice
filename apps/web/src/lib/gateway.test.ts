import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, api } from "./gateway";

function respond(status: number, body?: unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(body === undefined ? null : JSON.stringify(body), { status })),
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("api", () => {
  it("returns parsed JSON and always sends the session cookie", async () => {
    respond(200, { projects: [] });
    await expect(api("/api/projects")).resolves.toEqual({ projects: [] });
    const [, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(init?.credentials).toBe("include");
  });

  it("serialises bodies as JSON", async () => {
    respond(201, { id: "p1" });
    await api("/api/projects", { method: "POST", body: { name: "Revenue" } });
    const [, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(init?.headers).toEqual({ "content-type": "application/json" });
    expect(init?.body).toBe('{"name":"Revenue"}');
  });

  it("maps the gateway error envelope onto ApiError", async () => {
    respond(401, { error: { code: "invalid_credentials", message: "invalid email or password" } });
    const error = await api("/api/auth/login").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      status: 401,
      code: "invalid_credentials",
      message: "invalid email or password",
    });
    expect((error as ApiError).isUnauthenticated).toBe(true);
  });

  it("explains rate limiting even without a JSON body", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("Too Many Requests", { status: 429 })),
    );
    await expect(api("/api/auth/login")).rejects.toMatchObject({
      status: 429,
      message: expect.stringMatching(/Too many attempts/),
    });
  });

  it("turns network failures into a readable error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Promise.reject(new TypeError("Failed to fetch"))),
    );
    await expect(api("/api/projects")).rejects.toMatchObject({ status: 0, code: "network" });
  });

  it("returns undefined for 204", async () => {
    respond(204);
    await expect(api("/api/auth/logout", { method: "POST" })).resolves.toBeUndefined();
  });
});
