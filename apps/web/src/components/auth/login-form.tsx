"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useLogin } from "@/lib/queries";

import { Field, FormError, RememberMe, safeNextPath, useQueryParam } from "./form-bits";
import { SsoButtons } from "./sso-buttons";

const SSO_ERRORS: Record<string, string> = {
  sso_failed: "Single sign-on didn't complete. Try again, or sign in with your email.",
};

export function LoginForm() {
  const router = useRouter();
  const login = useLogin();
  const next = useQueryParam("next");
  const ssoError = useQueryParam("error");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    login.mutate(
      { email, password, remember },
      { onSuccess: () => router.replace(safeNextPath(next)) },
    );
  }

  const error =
    login.error?.message ?? (ssoError ? (SSO_ERRORS[ssoError] ?? SSO_ERRORS.sso_failed) : null);

  return (
    <>
      <div className="flex flex-col gap-2">
        <h2 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
          Sign in to your workspace
        </h2>
        <p className="text-[15px] text-muted-foreground">
          Welcome back. Use your company account or email.
        </p>
      </div>

      <SsoButtons remember={remember} />

      <div className="flex items-center gap-3 text-[13px] text-subtle-foreground">
        <div className="h-px flex-1 bg-border" />
        <span>or with email</span>
        <div className="h-px flex-1 bg-border" />
      </div>

      <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate={false}>
        <FormError message={error} />
        <Field id="email" label="Work email">
          <Input
            id="email"
            type="email"
            autoComplete="email"
            required
            placeholder="you@company.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="h-[46px]"
          />
        </Field>
        <Field
          id="password"
          label="Password"
          aside={
            <Tooltip>
              <TooltipTrigger asChild>
                <button type="button" className="text-[13px] text-primary hover:text-[#312e81]">
                  Forgot password?
                </button>
              </TooltipTrigger>
              <TooltipContent>
                Ask a workspace admin to reset it — self-service reset needs email delivery, which
                isn&apos;t set up yet.
              </TooltipContent>
            </Tooltip>
          }
        >
          <Input
            id="password"
            type="password"
            autoComplete="current-password"
            required
            placeholder="••••••••"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="h-[46px]"
          />
        </Field>
        <RememberMe checked={remember} onChange={setRemember} />
        <Button type="submit" size="lg" disabled={login.isPending}>
          {login.isPending ? "Signing in…" : "Sign in"}
        </Button>
      </form>

      <p className="text-center text-sm text-muted-foreground">
        New to Lattice?{" "}
        <Link href="/signup" className="font-medium no-underline">
          Create a workspace
        </Link>
      </p>
    </>
  );
}
