"use client";

import { usePathname, useRouter } from "next/navigation";
import { useEffect, type ReactNode } from "react";

import { ApiError } from "@/lib/gateway";
import { useMe } from "@/lib/queries";

/**
 * Sends signed-out visitors to /login. It doesn't hold the page back while `/me` loads:
 * pages fetch their own data in parallel and show skeletons, so a signed-in user never
 * waits for two sequential round trips.
 */
export function AuthGate({ children }: { children: ReactNode }) {
  const me = useMe();
  const router = useRouter();
  const pathname = usePathname();
  const signedOut = me.error instanceof ApiError && me.error.isUnauthenticated;

  useEffect(() => {
    if (signedOut) router.replace(`/login?next=${encodeURIComponent(pathname)}`);
  }, [signedOut, pathname, router]);

  return children;
}

/** True when an error just means "you're signed out" (the gate is already redirecting). */
export function isSignedOutError(error: unknown): boolean {
  return error instanceof ApiError && error.isUnauthenticated;
}
