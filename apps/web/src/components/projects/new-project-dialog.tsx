"use client";

import { useRouter } from "next/navigation";
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
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useCreateProject } from "@/lib/queries";

export function NewProjectDialog() {
  const router = useRouter();
  const create = useCreateProject();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    create.mutate(
      { name, description },
      {
        onSuccess: (project) => {
          setOpen(false);
          setName("");
          setDescription("");
          router.push(`/projects/${project.id}`);
        },
      },
    );
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) create.reset();
      }}
    >
      <DialogTrigger asChild>
        <Button>+ New project</Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-[480px]">
        <form onSubmit={onSubmit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>New project</DialogTitle>
            <DialogDescription>
              A project has its own sources, sheets and members. You&apos;ll be its owner.
            </DialogDescription>
          </DialogHeader>
          <FormError message={create.error?.message} />
          <Field id="project-name" label="Name">
            <Input
              id="project-name"
              required
              maxLength={80}
              autoFocus
              placeholder="e.g. Revenue Analytics"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field
            id="project-description"
            label="Description"
            hint="Optional. Who is it for, and what does it answer?"
          >
            <Input
              id="project-description"
              maxLength={500}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending || !name.trim()}>
              {create.isPending ? "Creating…" : "Create project"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
