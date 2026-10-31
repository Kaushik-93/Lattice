"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useSignup } from "@/lib/queries";

import { Field, FormError, RememberMe } from "./form-bits";

export function SignupForm() {
  const router = useRouter();
  const signup = useSignup();
  const [form, setForm] = useState({
    display_name: "",
    email: "",
    workspace_name: "",
    password: "",
  });
  const [remember, setRemember] = useState(true);
  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    signup.mutate({ ...form, remember }, { onSuccess: () => router.replace("/projects") });
  }

  return (
    <>
      <div className="flex flex-col gap-2">
        <h2 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
          Create a workspace
        </h2>
        <p className="text-[15px] text-muted-foreground">
          Your workspace holds your team&apos;s projects and the business logic they share.
        </p>
      </div>

      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <FormError message={signup.error?.message} />
        <Field id="display_name" label="Your name">
          <Input
            id="display_name"
            autoComplete="name"
            required
            value={form.display_name}
            onChange={set("display_name")}
            className="h-[46px]"
          />
        </Field>
        <Field id="email" label="Work email">
          <Input
            id="email"
            type="email"
            autoComplete="email"
            required
            placeholder="you@company.com"
            value={form.email}
            onChange={set("email")}
            className="h-[46px]"
          />
        </Field>
        <Field
          id="workspace_name"
          label="Workspace name"
          hint="Usually your company or team, e.g. “Acme Retail”."
        >
          <Input
            id="workspace_name"
            autoComplete="organization"
            required
            value={form.workspace_name}
            onChange={set("workspace_name")}
            className="h-[46px]"
          />
        </Field>
        <Field id="password" label="Password" hint="At least 10 characters.">
          <Input
            id="password"
            type="password"
            autoComplete="new-password"
            required
            minLength={10}
            value={form.password}
            onChange={set("password")}
            className="h-[46px]"
          />
        </Field>
        <RememberMe checked={remember} onChange={setRemember} />
        <Button type="submit" size="lg" disabled={signup.isPending}>
          {signup.isPending ? "Creating workspace…" : "Create workspace"}
        </Button>
      </form>

      <p className="text-center text-sm text-muted-foreground">
        Already have an account?{" "}
        <Link href="/login" className="font-medium no-underline">
          Sign in
        </Link>
      </p>
    </>
  );
}
