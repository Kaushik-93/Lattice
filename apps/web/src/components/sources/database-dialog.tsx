"use client";

import type { SourceDetail } from "@lattice/shared-types";
import { useState, type FormEvent } from "react";

import { Field, FormError } from "@/components/auth/form-bits";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useCreateDatabaseSource } from "@/lib/queries";

export type DatabaseKind = "postgres" | "mysql";

const DEFAULTS: Record<DatabaseKind, { label: string; port: number }> = {
  postgres: { label: "PostgreSQL", port: 5432 },
  mysql: { label: "MySQL", port: 3306 },
};

/** Connects a database as a live source. The gateway checks the credentials before saving. */
export function DatabaseDialog({
  projectId,
  kind,
  onClose,
  onCreated,
}: {
  projectId: string;
  kind: DatabaseKind | null;
  onClose: () => void;
  onCreated: (detail: SourceDetail) => void;
}) {
  const create = useCreateDatabaseSource(projectId);
  const [form, setForm] = useState({
    name: "",
    host: "",
    port: "",
    database: "",
    user: "",
    password: "",
    schema: "",
  });
  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));
  const label = kind ? DEFAULTS[kind].label : "";

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!kind) return;
    create.mutate(
      {
        kind,
        name: form.name || `${form.database} (${label})`,
        config: {
          host: form.host.trim(),
          port: Number(form.port || DEFAULTS[kind].port),
          database: form.database.trim(),
          user: form.user.trim(),
          schema: form.schema.trim() || null,
        },
        password: form.password,
      },
      {
        onSuccess: (detail) => {
          setForm({
            name: "",
            host: "",
            port: "",
            database: "",
            user: "",
            password: "",
            schema: "",
          });
          onCreated(detail);
        },
      },
    );
  }

  return (
    <Dialog
      open={kind !== null}
      onOpenChange={(open) => {
        if (!open) {
          create.reset();
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-[520px]">
        <form onSubmit={onSubmit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Connect {label}</DialogTitle>
            <DialogDescription>
              Queried live and read-only. The password is encrypted at rest and never shown again.
            </DialogDescription>
          </DialogHeader>
          <FormError message={create.error?.message} />
          <div className="grid grid-cols-[1fr_110px] gap-3">
            <Field id="db-host" label="Host">
              <Input
                id="db-host"
                required
                placeholder="db.internal"
                value={form.host}
                onChange={set("host")}
              />
            </Field>
            <Field id="db-port" label="Port">
              <Input
                id="db-port"
                inputMode="numeric"
                placeholder={kind ? String(DEFAULTS[kind].port) : ""}
                value={form.port}
                onChange={set("port")}
              />
            </Field>
          </div>
          <Field id="db-database" label="Database">
            <Input id="db-database" required value={form.database} onChange={set("database")} />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field id="db-user" label="User">
              <Input
                id="db-user"
                required
                autoComplete="off"
                value={form.user}
                onChange={set("user")}
              />
            </Field>
            <Field id="db-password" label="Password">
              <Input
                id="db-password"
                type="password"
                autoComplete="new-password"
                value={form.password}
                onChange={set("password")}
              />
            </Field>
          </div>
          {kind === "postgres" && (
            <Field id="db-schema" label="Schema" hint="Optional: only list tables in this schema.">
              <Input
                id="db-schema"
                placeholder="public"
                value={form.schema}
                onChange={set("schema")}
              />
            </Field>
          )}
          <Field id="db-name" label="Display name" hint="Optional. Defaults to the database name.">
            <Input id="db-name" value={form.name} onChange={set("name")} />
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? "Connecting…" : "Connect"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
