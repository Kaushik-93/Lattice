"use client";

import { useSyncExternalStore, type ReactNode } from "react";

import { cn } from "@/lib/utils";

export function FormError({ message }: { message: string | null | undefined }) {
  if (!message) return null;
  return (
    <p
      role="alert"
      className="rounded-[10px] border border-[#f5c2bd] bg-destructive-surface px-3.5 py-2.5 text-sm text-destructive"
    >
      {message}
    </p>
  );
}

export function Field({
  id,
  label,
  aside,
  hint,
  children,
}: {
  id: string;
  label: string;
  aside?: ReactNode;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between">
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        {aside}
      </div>
      {children}
      {hint && <span className="text-xs text-subtle-foreground">{hint}</span>}
    </div>
  );
}

export function RememberMe({
  checked,
  onChange,
  className,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  className?: string;
}) {
  return (
    <label className={cn("flex items-center gap-2 text-sm text-secondary-foreground", className)}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="size-4 accent-primary"
      />
      Keep me signed in for 30 days
    </label>
  );
}

const noSubscription = () => () => {};

/**
 * Reads one-shot query params (`?next=`, `?error=`) straight from `location` instead of
 * useSearchParams, so the form itself stays in the prerendered HTML and is interactive as
 * soon as it hydrates — no Suspense fallback in front of the sign-in form.
 */
export function useQueryParam(name: string): string | null {
  return useSyncExternalStore(
    noSubscription,
    () => new URLSearchParams(window.location.search).get(name),
    () => null,
  );
}

/** Only same-origin paths: `?next=//evil.example` must not become an open redirect. */
export function safeNextPath(next: string | null): string {
  if (next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\"))
    return next;
  return "/projects";
}
