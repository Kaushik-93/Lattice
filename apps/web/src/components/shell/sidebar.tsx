"use client";

import {
  ChevronDown,
  Database,
  Gauge,
  LayoutGrid,
  LogOut,
  Settings,
  Share2,
  Table2,
} from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import type { ReactNode } from "react";

import { Logo } from "@/components/brand/logo";
import { ProjectTile } from "@/components/projects/project-tile";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { capitalize } from "@/lib/format";
import { useLogout, useMe, useProjects } from "@/lib/queries";
import { useUiStore } from "@/lib/ui-store";
import { cn } from "@/lib/utils";

import { FolderIcon, LogicIcon, PeopleIcon } from "./icons";

const PROJECT_PATH = /^\/projects\/([^/]+)/;

export function Sidebar() {
  const pathname = usePathname();
  const filter = useSearchParams().get("filter");
  const projectId = PROJECT_PATH.exec(pathname)?.[1];
  const projects = useProjects();
  const current = projectId ? projects.data?.find((p) => p.id === projectId) : undefined;
  const recent = (projects.data ?? []).slice(0, 3);

  return (
    <SidebarFrame>
      <NavItem
        href="/projects"
        icon={<FolderIcon />}
        active={pathname === "/projects" && filter !== "shared"}
      >
        All projects
      </NavItem>
      <NavItem
        href="/projects?filter=shared"
        icon={<PeopleIcon />}
        active={pathname === "/projects" && filter === "shared"}
      >
        Shared with me
      </NavItem>
      <NavItem href="/logic" icon={<LogicIcon />} active={pathname.startsWith("/logic")}>
        Business logic
      </NavItem>

      {projectId ? (
        <>
          <SectionLabel>Project</SectionLabel>
          <div className="mb-1.5 flex h-11 items-center gap-2.5 rounded-[10px] border border-border bg-[#fafbfc] px-2.5">
            {current ? (
              <>
                <ProjectTile name={current.name} color={current.color} size={22} />
                <span className="flex-1 truncate text-sm font-semibold">{current.name}</span>
              </>
            ) : (
              <Skeleton className="h-4 w-32" />
            )}
            <ChevronDown className="size-3.5 flex-none text-subtle-foreground" aria-hidden />
          </div>
          <NavItem
            href={`/projects/${projectId}`}
            icon={<LayoutGrid className="size-[18px]" />}
            active={pathname === `/projects/${projectId}`}
          >
            Overview
          </NavItem>
          <NavItem href={`/projects/${projectId}#sheets`} icon={<Table2 className="size-[18px]" />}>
            Sheets
          </NavItem>
          <NavItem
            href={`/projects/${projectId}/sources`}
            icon={<Database className="size-[18px]" />}
            active={pathname.startsWith(`/projects/${projectId}/sources`)}
          >
            Sources
          </NavItem>
          <NavItem
            href={`/projects/${projectId}#members`}
            icon={<Share2 className="size-[18px]" />}
          >
            Members &amp; sharing
          </NavItem>
        </>
      ) : (
        recent.length > 0 && (
          <>
            <SectionLabel>Recent</SectionLabel>
            {recent.map((p) => (
              <Link
                key={p.id}
                href={`/projects/${p.id}`}
                className="flex h-[38px] items-center gap-2.5 rounded-lg px-2.5 text-sm text-secondary-foreground no-underline hover:bg-secondary hover:text-foreground"
              >
                <ProjectTile name={p.name} color={p.color} size={20} />
                <span className="truncate">{p.name}</span>
              </Link>
            ))}
          </>
        )
      )}
      <div className="flex-1" />
      <UserMenu />
    </SidebarFrame>
  );
}

/** Prerendered placeholder while the sidebar's URL-dependent parts resolve. */
export function SidebarFallback() {
  return (
    <SidebarFrame>
      {["All projects", "Shared with me", "Business logic"].map((label) => (
        <div
          key={label}
          className="flex h-10 items-center px-2.5 text-sm font-medium text-secondary-foreground"
        >
          {label}
        </div>
      ))}
    </SidebarFrame>
  );
}

function SidebarFrame({ children }: { children: ReactNode }) {
  return (
    <nav
      aria-label="Primary"
      className="flex max-w-full flex-[1_1_232px] flex-col gap-0.5 border-r border-border bg-card px-3.5 py-5 md:sticky md:top-0 md:h-screen md:max-w-[232px]"
    >
      <Link
        href="/projects"
        className="px-2.5 pt-1 pb-4 text-foreground no-underline hover:text-foreground"
      >
        <Logo />
      </Link>
      {children}
    </nav>
  );
}

function NavItem({
  href,
  icon,
  active = false,
  children,
}: {
  href: string;
  icon: ReactNode;
  active?: boolean;
  children: ReactNode;
}) {
  return (
    <Link
      href={href}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-10 items-center gap-2.5 rounded-lg px-2.5 text-sm no-underline",
        active
          ? "bg-accent font-semibold text-accent-foreground hover:text-accent-foreground"
          : "font-medium text-secondary-foreground hover:bg-secondary hover:text-foreground",
      )}
    >
      {icon}
      {children}
    </Link>
  );
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="mt-[18px] mb-1.5 px-2.5 text-[11px] font-semibold tracking-[0.06em] text-subtle-foreground uppercase">
      {children}
    </div>
  );
}

function UserMenu() {
  const me = useMe();
  const logout = useLogout();
  const router = useRouter();
  const togglePerf = useUiStore((s) => s.togglePerfOverlay);

  if (!me.data) {
    return (
      <div className="flex items-center gap-2.5 border-t border-border px-2.5 py-3">
        <Skeleton className="size-8 rounded-full" />
        <div className="flex flex-col gap-1.5">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-3 w-32" />
        </div>
      </div>
    );
  }
  const { user, workspace, workspace_role } = me.data;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="flex items-center gap-2.5 border-t border-border px-2.5 py-3 text-left outline-none hover:bg-secondary focus-visible:ring-2 focus-visible:ring-ring">
        <span className="flex size-8 flex-none items-center justify-center rounded-full bg-avatar text-xs font-semibold text-avatar-foreground">
          {user.initials}
        </span>
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-[13px] font-medium">{user.display_name}</span>
          <span className="truncate text-xs text-subtle-foreground">
            {capitalize(workspace_role)} · {workspace.name}
          </span>
        </span>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-56">
        <DropdownMenuLabel className="truncate font-normal text-subtle-foreground">
          {user.email}
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => router.push("/settings")}>
          <Settings /> Settings
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={togglePerf}>
          <Gauge /> Performance overlay
          <span className="ml-auto text-xs text-subtle-foreground">Ctrl⇧P</span>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => logout.mutate(undefined, { onSettled: () => router.replace("/login") })}
        >
          <LogOut /> Sign out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
