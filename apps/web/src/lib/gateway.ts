import type { ApiErrorBody } from "@lattice/shared-types";

/**
 * The browser talks to the Rust gateway directly — never through Next.js route handlers —
 * so no request pays an extra hop (spec §2.1). Inlined at build time.
 */
export const GATEWAY_URL = (process.env.NEXT_PUBLIC_GATEWAY_URL ?? "http://localhost:8080").replace(
  /\/$/,
  "",
);

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }

  get isUnauthenticated(): boolean {
    return this.status === 401;
  }
}

interface RequestOptions {
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  body?: unknown;
  signal?: AbortSignal;
}

function isApiErrorBody(value: unknown): value is ApiErrorBody {
  if (typeof value !== "object" || value === null || !("error" in value)) return false;
  const error = (value as { error: unknown }).error;
  return (
    typeof error === "object" &&
    error !== null &&
    typeof (error as { code?: unknown }).code === "string" &&
    typeof (error as { message?: unknown }).message === "string"
  );
}

/** Calls a gateway REST endpoint with the session cookie; throws {@link ApiError} on failure. */
export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const hasBody = options.body !== undefined;
  let response: Response;
  try {
    response = await fetch(`${GATEWAY_URL}${path}`, {
      method: options.method ?? "GET",
      credentials: "include",
      headers: hasBody ? { "content-type": "application/json" } : undefined,
      body: hasBody ? JSON.stringify(options.body) : undefined,
      signal: options.signal,
    });
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === "AbortError") throw cause;
    throw new ApiError(0, "network", "Can't reach Lattice. Check your connection and try again.");
  }

  if (response.status === 204) return undefined as T;
  const text = await response.text();
  let data: unknown;
  try {
    data = text ? JSON.parse(text) : undefined;
  } catch {
    data = undefined;
  }

  if (!response.ok) {
    if (isApiErrorBody(data))
      throw new ApiError(response.status, data.error.code, data.error.message);
    const message =
      response.status === 429
        ? "Too many attempts. Wait a moment and try again."
        : `Request failed (${response.status}).`;
    throw new ApiError(response.status, `http_${response.status}`, message);
  }
  // Gateway payloads are typed by the generated contracts in @lattice/shared-types.
  return data as T;
}
