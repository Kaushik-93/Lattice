"use client";

import type { LinkAudience, ProjectRole, SheetSharing, SharingUpdate } from "@lattice/shared-types";
import { Eye, X } from "lucide-react";
import Link from "next/link";
import { useParams, useSearchParams } from "next/navigation";
import { useId, useState, type FormEvent } from "react";

import { isSignedOutError } from "@/components/shell/auth-gate";
import { ErrorState, PageSkeleton } from "@/components/shell/page-skeleton";
import { Button } from "@/components/ui/button";
import { capitalize } from "@/lib/format";
import { ApiError } from "@/lib/gateway";
import {
  useAddProjectMember,
  useMe,
  useProject,
  useRemoveProjectMember,
  useUpdateProjectMember,
} from "@/lib/queries";
import {
  useInviteToSheet,
  useRemoveFromSheet,
  useSharing,
  useSheets,
  useUpdateSharing,
} from "@/lib/sheets/api";
import { cn } from "@/lib/utils";

const ROLES: ProjectRole[] = ["viewer", "editor", "admin"];

/** The design's role matrix, as the gateway enforces it today. */
const PERMISSIONS: [string, boolean, boolean][] = [
  ["View all sheets and sources", true, true],
  ["Create & edit sheets", false, true],
  ["Share sheets by link", false, true],
  ["Manage sources & members", false, false],
];

const AUDIENCES: [Exclude<LinkAudience, "off">, string, string][] = [
  ["invited", "Only invited people", "Must be added by name and signed in"],
  ["workspace", "Anyone at {workspace} with the link", "Signed in with a company account"],
  ["public", "Anyone with the link", "No sign-in needed — good for partners and board members"],
];

const card = "flex flex-col gap-4 rounded-xl border border-border bg-card p-6";
const input =
  "h-11 min-w-0 flex-1 rounded-[10px] border border-input bg-card px-3.5 text-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

function errorText(error: unknown): string | null {
  return error instanceof ApiError || error instanceof Error ? error.message : null;
}

/** Design screen 07: project members, and links to single sheets. */
export function SharingView() {
  const { projectId } = useParams<{ projectId: string }>();
  const params = useSearchParams();
  const project = useProject(projectId);
  const [scope, setScope] = useState<"project" | "sheet">(
    params.get("sheet") ? "sheet" : "project",
  );

  if (project.isPending || isSignedOutError(project.error)) return <PageSkeleton />;
  if (project.error)
    return <ErrorState title="Couldn't load this project" message={project.error.message} />;
  const { project: summary } = project.data;
  const isAdmin = summary.my_role === "admin";
  const canShare = summary.my_role !== "viewer";

  return (
    <>
      <header className="flex flex-col gap-1.5">
        <span className="text-[13px] text-subtle-foreground">
          <Link
            href="/projects"
            className="text-subtle-foreground no-underline hover:text-foreground"
          >
            Projects
          </Link>{" "}
          /{" "}
          <Link
            href={`/projects/${projectId}`}
            className="text-subtle-foreground no-underline hover:text-foreground"
          >
            {summary.name}
          </Link>{" "}
          / Sharing
        </span>
        <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
          Members &amp; sharing
        </h1>
        <p className="max-w-[640px] text-[15px] text-muted-foreground">
          Give employees access to the whole project, or share a single sheet — with specific people
          or through a link.
        </p>
      </header>
      <div
        role="tablist"
        aria-label="What to share"
        className="flex w-fit rounded-xl bg-secondary p-1"
      >
        {(
          [
            ["project", "Whole project"],
            ["sheet", "A single sheet"],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={scope === key}
            onClick={() => setScope(key)}
            className={cn(
              "h-10 rounded-[10px] px-5 text-sm font-semibold",
              scope === key
                ? "bg-card text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {label}
          </button>
        ))}
      </div>
      {scope === "project" ? (
        <ProjectMembers projectId={projectId} projectName={summary.name} isAdmin={isAdmin} />
      ) : (
        <SheetLinks projectId={projectId} canShare={canShare} initial={params.get("sheet")} />
      )}
    </>
  );
}

function ProjectMembers({
  projectId,
  projectName,
  isAdmin,
}: {
  projectId: string;
  projectName: string;
  isAdmin: boolean;
}) {
  const project = useProject(projectId);
  const me = useMe();
  const add = useAddProjectMember(projectId);
  const update = useUpdateProjectMember(projectId);
  const remove = useRemoveProjectMember(projectId);
  const emailId = useId();
  const roleId = useId();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<ProjectRole>("viewer");
  const members = project.data?.members ?? [];
  const error = errorText(add.error ?? update.error ?? remove.error);

  const invite = (e: FormEvent) => {
    e.preventDefault();
    if (!email.trim()) return;
    add.mutate({ email: email.trim(), role }, { onSuccess: () => setEmail("") });
  };

  return (
    <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,420px),1fr))] items-start gap-5">
      <section aria-labelledby="inv-h" className={card}>
        <div className="flex flex-col gap-1">
          <h2 id="inv-h" className="text-lg font-semibold">
            Share project with employees
          </h2>
          <p className="text-sm text-muted-foreground">
            Members see every sheet in {projectName}, plus sources according to their role.
          </p>
        </div>
        {isAdmin && (
          <form onSubmit={invite} className="flex flex-wrap items-end gap-2">
            <label htmlFor={emailId} className="sr-only">
              Email of someone in your workspace
            </label>
            <input
              id={emailId}
              type="email"
              className={input}
              placeholder="name@company.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
            <label htmlFor={roleId} className="sr-only">
              Role
            </label>
            <select
              id={roleId}
              className={cn(input, "w-28 flex-none")}
              value={role}
              onChange={(e) => setRole(e.target.value as ProjectRole)}
            >
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {capitalize(r)}
                </option>
              ))}
            </select>
            <Button type="submit" className="h-11" disabled={add.isPending}>
              Invite
            </Button>
          </form>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <div className="flex flex-col">
          <h3 className="mb-1 text-[13px] font-semibold text-secondary-foreground">
            People with access · {members.length}
          </h3>
          {members.map((m) => (
            <div
              key={m.user_id}
              className="flex items-center gap-3 border-b border-border-subtle py-2.5 last:border-0"
            >
              <span className="grid size-9 flex-none place-items-center rounded-full bg-avatar text-xs font-semibold text-accent-foreground">
                {m.initials}
              </span>
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm font-medium">
                  {m.display_name}
                  {m.user_id === me.data?.user.id ? " (you)" : ""}
                </span>
                <span className="truncate text-xs text-subtle-foreground">{m.email}</span>
              </div>
              {m.is_owner || !isAdmin ? (
                <span className="text-sm text-secondary-foreground">
                  {m.is_owner ? "Owner" : capitalize(m.role)}
                </span>
              ) : (
                <>
                  <select
                    aria-label={`Role of ${m.display_name}`}
                    className="h-9 rounded-md border border-input bg-card px-2 text-sm"
                    value={m.role}
                    onChange={(e) =>
                      update.mutate({ userId: m.user_id, role: e.target.value as ProjectRole })
                    }
                  >
                    {ROLES.map((r) => (
                      <option key={r} value={r}>
                        {capitalize(r)}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    aria-label={`Remove ${m.display_name}`}
                    onClick={() => remove.mutate(m.user_id)}
                    className="grid size-8 place-items-center rounded-md text-subtle-foreground hover:bg-secondary hover:text-foreground"
                  >
                    <X className="size-4" />
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      </section>
      <section aria-labelledby="roles-h" className={card}>
        <h2 id="roles-h" className="text-lg font-semibold">
          What each role can do
        </h2>
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border text-left text-secondary-foreground">
              <th className="py-2 font-medium">Permission</th>
              <th className="py-2 text-center font-medium">Viewer</th>
              <th className="py-2 text-center font-medium">Editor</th>
              <th className="py-2 text-center font-medium">Admin</th>
            </tr>
          </thead>
          <tbody>
            {PERMISSIONS.map(([name, viewer, editor]) => (
              <tr key={name} className="border-b border-border-subtle last:border-0">
                <td className="py-2.5">{name}</td>
                {[viewer, editor, true].map((ok, i) => (
                  <td
                    key={i}
                    className={cn(
                      "py-2.5 text-center",
                      ok ? "text-success" : "text-subtle-foreground",
                    )}
                  >
                    {ok ? "✓" : "—"}
                    <span className="sr-only">{ok ? "allowed" : "not allowed"}</span>
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}

function stateLabel(audience: LinkAudience): string {
  return audience === "off" ? "Members only" : "View link on";
}

function SheetLinks({
  projectId,
  canShare,
  initial,
}: {
  projectId: string;
  canShare: boolean;
  initial: string | null;
}) {
  const sheets = useSheets(projectId);
  const [picked, setPicked] = useState<string | null>(initial);
  const sheetId = picked ?? sheets.data?.[0]?.id ?? null;
  const sheet = sheets.data?.find((s) => s.id === sheetId);
  if (sheets.data?.length === 0) {
    return <ErrorState title="No sheets yet" message="Create a sheet first, then share it here." />;
  }
  return (
    <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,420px),1fr))] items-start gap-5">
      <section aria-labelledby="pick-h" className={card}>
        <h2 id="pick-h" className="text-lg font-semibold">
          Choose a sheet
        </h2>
        <div role="radiogroup" aria-labelledby="pick-h" className="flex flex-col gap-2.5">
          {(sheets.data ?? []).map((s) => (
            <button
              key={s.id}
              type="button"
              role="radio"
              aria-checked={s.id === sheetId}
              onClick={() => setPicked(s.id)}
              className={cn(
                "flex h-12 items-center gap-3 rounded-[10px] border px-4 text-left",
                s.id === sheetId
                  ? "border-primary bg-[#f7f7ff]"
                  : "border-input hover:border-subtle-foreground",
              )}
            >
              <span
                className={cn(
                  "grid size-4 place-items-center rounded-full border",
                  s.id === sheetId ? "border-primary" : "border-input",
                )}
              >
                {s.id === sheetId && <span className="size-2 rounded-full bg-primary" />}
              </span>
              <span className="flex-1 text-sm font-medium">{s.name}</span>
              <span className="text-xs text-muted-foreground">{stateLabel(s.link_audience)}</span>
            </button>
          ))}
        </div>
        {sheetId && canShare && <SheetInvites projectId={projectId} sheetId={sheetId} />}
      </section>
      {sheetId && sheet && (
        <LinkSettings
          key={sheetId}
          projectId={projectId}
          sheetId={sheetId}
          sheetName={sheet.name}
          canShare={canShare}
        />
      )}
    </div>
  );
}

function SheetInvites({ projectId, sheetId }: { projectId: string; sheetId: string }) {
  const sharing = useSharing(projectId, sheetId);
  const invite = useInviteToSheet(projectId, sheetId);
  const remove = useRemoveFromSheet(projectId, sheetId);
  const id = useId();
  const [email, setEmail] = useState("");
  const error = errorText(invite.error ?? remove.error);
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={id} className="text-[13px] font-semibold text-secondary-foreground">
        Invite specific people to this sheet only
      </label>
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (email.trim()) invite.mutate(email.trim(), { onSuccess: () => setEmail("") });
        }}
      >
        <input
          id={id}
          type="email"
          className={input}
          placeholder="name@company.com"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <Button type="submit" variant="outline" className="h-11" disabled={invite.isPending}>
          Invite
        </Button>
      </form>
      <span className="text-xs text-muted-foreground">
        They won&apos;t see other sheets or sources.
      </span>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {(sharing.data?.members ?? []).map((m) => (
        <div key={m.user_id} className="flex items-center gap-2 text-sm">
          <span className="flex-1 truncate">
            {m.display_name} <span className="text-subtle-foreground">{m.email}</span>
          </span>
          <button
            type="button"
            aria-label={`Remove ${m.display_name}`}
            onClick={() => remove.mutate(m.user_id)}
            className="grid size-7 place-items-center rounded-md hover:bg-secondary"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}

function expiryDays(s: SheetSharing): number | null {
  if (!s.expires_at) return null;
  const days = Math.round((Date.parse(s.expires_at) - Date.now()) / 86_400_000);
  return days <= 7 ? 7 : 30;
}

function LinkSettings({
  projectId,
  sheetId,
  sheetName,
  canShare,
}: {
  projectId: string;
  sheetId: string;
  sheetName: string;
  canShare: boolean;
}) {
  const sharing = useSharing(projectId, sheetId);
  const update = useUpdateSharing(projectId, sheetId);
  const me = useMe();
  const expiresId = useId();
  const urlId = useId();
  const [copied, setCopied] = useState(false);
  const s = sharing.data;
  if (!s) return <section className={cn(card, "min-h-80")} aria-busy="true" />;

  const apply = (patch: Partial<SharingUpdate>) =>
    update.mutate({
      audience: s.audience,
      expires_in_days: expiryDays(s),
      allow_export: s.allow_export,
      track_views: s.track_views,
      allow_copilot: s.allow_copilot,
      rotate: false,
      ...patch,
    });
  const url = s.token ? `${window.location.origin}/s/${s.token}` : "";
  const workspace = me.data?.workspace.name ?? "your workspace";
  const disabled = !canShare || update.isPending;

  return (
    <section aria-labelledby="link-h" className={card}>
      <h2 id="link-h" className="text-lg font-semibold">
        Link to “{sheetName}”
      </h2>
      <fieldset className="flex flex-col gap-2.5" disabled={disabled}>
        <legend className="mb-2 text-[13px] font-semibold text-secondary-foreground">
          Who can open the link
        </legend>
        {AUDIENCES.map(([audience, label, hint]) => {
          const on = s.audience === audience;
          return (
            <button
              key={audience}
              type="button"
              aria-pressed={on}
              onClick={() =>
                apply({ audience, expires_in_days: s.audience === "off" ? 30 : expiryDays(s) })
              }
              className={cn(
                "flex items-start gap-3 rounded-[10px] border px-4 py-3 text-left",
                on ? "border-primary bg-[#f7f7ff]" : "border-input hover:border-subtle-foreground",
              )}
            >
              <span
                className={cn(
                  "mt-0.5 grid size-4 flex-none place-items-center rounded-full border",
                  on ? "border-primary" : "border-input",
                )}
              >
                {on && <span className="size-2 rounded-full bg-primary" />}
              </span>
              <span className="flex flex-col">
                <span className="text-sm font-medium">
                  {label.replace("{workspace}", workspace)}
                </span>
                <span className="text-xs text-muted-foreground">{hint}</span>
              </span>
            </button>
          );
        })}
      </fieldset>
      <div className="flex gap-3 rounded-[10px] bg-secondary px-4 py-3">
        <Eye className="mt-0.5 size-4 flex-none text-secondary-foreground" aria-hidden />
        <span className="flex flex-col gap-0.5">
          <span className="text-sm font-semibold">Links always give view access</span>
          <span className="text-xs text-secondary-foreground">
            People with the link can read the sheet and use its filters. They can&apos;t edit, save,
            or open other sheets in the project.
          </span>
        </span>
      </div>
      {s.audience !== "off" && (
        <>
          <div className="flex flex-col">
            <Toggle
              label="Allow export"
              hint="Download visuals as CSV or PNG"
              on={s.allow_export}
              disabled={disabled}
              onChange={(v) => apply({ allow_export: v })}
            />
            <Toggle
              label="Track who viewed"
              hint={`Count opens of this link · ${s.view_count.toLocaleString("en-US")} so far`}
              on={s.track_views}
              disabled={disabled}
              onChange={(v) => apply({ track_views: v })}
            />
            <Toggle
              label="Let viewers ask the copilot"
              hint="Arrives with the AI copilot"
              on={false}
              disabled
              onChange={() => undefined}
            />
            <div className="flex items-center justify-between border-t border-border-subtle py-3">
              <label htmlFor={expiresId} className="text-sm font-medium">
                Link expires
              </label>
              <select
                id={expiresId}
                disabled={disabled}
                className="h-9 rounded-md border border-input bg-card px-2 text-sm"
                value={String(expiryDays(s) ?? "never")}
                onChange={(e) =>
                  apply({
                    expires_in_days: e.target.value === "never" ? null : Number(e.target.value),
                  })
                }
              >
                <option value="30">In 30 days</option>
                <option value="7">In 7 days</option>
                <option value="never">Never</option>
              </select>
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <label htmlFor={urlId} className="sr-only">
              Share link
            </label>
            <div className="flex gap-2">
              <input
                id={urlId}
                readOnly
                value={url}
                className={cn(input, "bg-[#fafbfc] font-mono text-[13px]")}
                onFocus={(e) => e.currentTarget.select()}
              />
              <Button
                className="h-11"
                onClick={() => {
                  void navigator.clipboard?.writeText(url).then(() => setCopied(true));
                }}
              >
                {copied ? "Copied" : "Copy link"}
              </Button>
            </div>
            <div className="flex flex-wrap gap-4 text-sm">
              <a href={url} target="_blank" rel="noreferrer">
                Preview as a viewer →
              </a>
              {canShare && (
                <>
                  <button
                    type="button"
                    className="text-primary hover:underline"
                    onClick={() => apply({ rotate: true })}
                  >
                    Get a new link
                  </button>
                  <button
                    type="button"
                    className="text-destructive hover:underline"
                    onClick={() => apply({ audience: "off", expires_in_days: null })}
                  >
                    Turn off link
                  </button>
                </>
              )}
            </div>
          </div>
        </>
      )}
      {errorText(update.error) && (
        <p role="alert" className="text-sm text-destructive">
          {errorText(update.error)}
        </p>
      )}
    </section>
  );
}

function Toggle({
  label,
  hint,
  on,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  on: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 border-t border-border-subtle py-3 first:border-0">
      <span className="flex flex-col">
        <span className="text-sm font-medium">{label}</span>
        <span className="text-xs text-muted-foreground">{hint}</span>
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!on)}
        className={cn(
          "flex h-6 w-11 flex-none rounded-full p-0.5 transition-colors disabled:opacity-50",
          on ? "justify-end bg-primary" : "justify-start bg-[#c3c8d4]",
        )}
      >
        <span className="size-5 rounded-full bg-white shadow" />
      </button>
    </div>
  );
}
