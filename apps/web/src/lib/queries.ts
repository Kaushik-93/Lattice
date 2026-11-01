"use client";

import type {
  AddProjectMemberRequest,
  AuthProvidersResponse,
  ProjectRole,
  CreateDatabaseSourceRequest,
  SourceDetail,
  SourceListResponse,
  CreateProjectRequest,
  LoginRequest,
  MeResponse,
  ProjectDetail,
  ProjectListResponse,
  ProjectSummary,
  SignupRequest,
} from "@lattice/shared-types";
import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";

import { ApiError, api } from "@/lib/gateway";

export const queryKeys = {
  me: ["me"] as const,
  providers: ["auth", "providers"] as const,
  projects: ["projects"] as const,
  project: (id: string) => ["projects", id] as const,
  sources: (projectId: string) => ["projects", projectId, "sources"] as const,
  source: (projectId: string, sourceId: string) =>
    ["projects", projectId, "sources", sourceId] as const,
};

/** Don't retry what retrying can't fix. */
function retryUnlessClientError(failureCount: number, error: Error): boolean {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false;
  return failureCount < 2;
}

export function useMe() {
  return useQuery({
    queryKey: queryKeys.me,
    queryFn: ({ signal }) => api<MeResponse>("/api/auth/me", { signal }),
    retry: retryUnlessClientError,
    staleTime: 5 * 60_000,
  });
}

export function useAuthProviders() {
  return useQuery({
    queryKey: queryKeys.providers,
    queryFn: ({ signal }) => api<AuthProvidersResponse>("/api/auth/providers", { signal }),
    staleTime: Infinity,
  });
}

export function useProjects() {
  return useQuery({
    queryKey: queryKeys.projects,
    queryFn: ({ signal }) => api<ProjectListResponse>("/api/projects", { signal }),
    select: (data) => data.projects,
    retry: retryUnlessClientError,
  });
}

function fetchProject(id: string, signal?: AbortSignal) {
  return api<ProjectDetail>(`/api/projects/${encodeURIComponent(id)}`, { signal });
}

export function useProject(id: string) {
  return useQuery({
    queryKey: queryKeys.project(id),
    queryFn: ({ signal }) => fetchProject(id, signal),
    retry: retryUnlessClientError,
  });
}

/** Warms the cache on hover/focus so opening a project renders without waiting on the network. */
export function prefetchProject(client: QueryClient, id: string): void {
  void client.prefetchQuery({
    queryKey: queryKeys.project(id),
    queryFn: ({ signal }) => fetchProject(id, signal),
    staleTime: 30_000,
  });
}

export function useLogin() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: LoginRequest) =>
      api<MeResponse>("/api/auth/login", { method: "POST", body }),
    onSuccess: (me) => {
      client.removeQueries({ queryKey: queryKeys.projects });
      client.setQueryData(queryKeys.me, me);
    },
  });
}

export function useSignup() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: SignupRequest) =>
      api<MeResponse>("/api/auth/signup", { method: "POST", body }),
    onSuccess: (me) => {
      client.removeQueries({ queryKey: queryKeys.projects });
      client.setQueryData(queryKeys.me, me);
    },
  });
}

export function useLogout() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api<void>("/api/auth/logout", { method: "POST" }),
    onSettled: () => client.clear(),
  });
}

export function useCreateProject() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateProjectRequest) =>
      api<ProjectSummary>("/api/projects", { method: "POST", body }),
    onSuccess: (project) => {
      client.setQueryData<ProjectListResponse>(queryKeys.projects, (old) =>
        old ? { projects: [project, ...old.projects] } : old,
      );
      void client.invalidateQueries({ queryKey: queryKeys.projects });
    },
  });
}

export function useSources(projectId: string) {
  return useQuery({
    queryKey: queryKeys.sources(projectId),
    queryFn: ({ signal }) =>
      api<SourceListResponse>(`/api/projects/${encodeURIComponent(projectId)}/sources`, { signal }),
    select: (data) => data.sources,
    retry: retryUnlessClientError,
  });
}

export function useSource(projectId: string, sourceId: string) {
  return useQuery({
    queryKey: queryKeys.source(projectId, sourceId),
    queryFn: ({ signal }) =>
      api<SourceDetail>(
        `/api/projects/${encodeURIComponent(projectId)}/sources/${encodeURIComponent(sourceId)}`,
        { signal },
      ),
    retry: retryUnlessClientError,
  });
}

/** After a source changes: refresh its detail, the source list, and project counts. */
function sourceChanged(client: QueryClient, projectId: string, detail?: SourceDetail) {
  if (detail) client.setQueryData(queryKeys.source(projectId, detail.source.id), detail);
  void client.invalidateQueries({ queryKey: queryKeys.sources(projectId) });
  void client.invalidateQueries({ queryKey: queryKeys.projects, exact: true });
  void client.invalidateQueries({ queryKey: queryKeys.project(projectId), exact: true });
}

export function useCreateDatabaseSource(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateDatabaseSourceRequest) =>
      api<SourceDetail>(`/api/projects/${encodeURIComponent(projectId)}/sources`, {
        method: "POST",
        body,
      }),
    onSuccess: (detail) => sourceChanged(client, projectId, detail),
  });
}

export function useRefreshSource(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (sourceId: string) =>
      api<SourceDetail>(
        `/api/projects/${encodeURIComponent(projectId)}/sources/${encodeURIComponent(sourceId)}/refresh`,
        { method: "POST" },
      ),
    onSuccess: (detail) => sourceChanged(client, projectId, detail),
  });
}

export function useDeleteSource(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (sourceId: string) =>
      api<void>(
        `/api/projects/${encodeURIComponent(projectId)}/sources/${encodeURIComponent(sourceId)}`,
        { method: "DELETE" },
      ),
    onSuccess: () => sourceChanged(client, projectId),
  });
}

export function useProfileTable(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (tableId: string) =>
      api<SourceDetail>(
        `/api/projects/${encodeURIComponent(projectId)}/tables/${encodeURIComponent(tableId)}/profile`,
        { method: "POST" },
      ),
    onSuccess: (detail) => sourceChanged(client, projectId, detail),
  });
}

function memberMutation<V>(projectId: string, request: (v: V) => Promise<ProjectDetail>) {
  return function useMemberMutation() {
    const client = useQueryClient();
    return useMutation({
      mutationFn: request,
      onSuccess: (detail) => {
        client.setQueryData(queryKeys.project(projectId), detail);
        void client.invalidateQueries({ queryKey: queryKeys.projects, exact: true });
      },
    });
  };
}

/** Gives someone in the workspace a role in the project (or changes it). */
export function useAddProjectMember(projectId: string) {
  return memberMutation(projectId, (req: AddProjectMemberRequest) =>
    api<ProjectDetail>(`/api/projects/${projectId}/members`, { method: "POST", body: req }),
  )();
}

export function useUpdateProjectMember(projectId: string) {
  return memberMutation(projectId, ({ userId, role }: { userId: string; role: ProjectRole }) =>
    api<ProjectDetail>(`/api/projects/${projectId}/members/${userId}`, {
      method: "PATCH",
      body: { role },
    }),
  )();
}

export function useRemoveProjectMember(projectId: string) {
  return memberMutation(projectId, (userId: string) =>
    api<ProjectDetail>(`/api/projects/${projectId}/members/${userId}`, { method: "DELETE" }),
  )();
}

export interface CatalogTable {
  id: string;
  name: string;
  source: string;
  rowCount: number | null;
  columns: { name: string; data_type: string; min: string | null; max: string | null }[];
}

/** Every table in the project with its columns (the visual editor's field lists). */
export function useProjectTables(projectId: string): { tables: CatalogTable[]; loading: boolean } {
  const sources = useSources(projectId);
  const details = useQueries({
    queries: (sources.data ?? []).map((s) => ({
      queryKey: queryKeys.source(projectId, s.id),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        api<SourceDetail>(
          `/api/projects/${encodeURIComponent(projectId)}/sources/${encodeURIComponent(s.id)}`,
          { signal },
        ),
      staleTime: 60_000,
    })),
  });
  const tables = details.flatMap((d) =>
    (d.data?.tables ?? []).map((t) => ({
      id: t.id,
      name: t.name,
      source: d.data?.source.name ?? "",
      rowCount: t.row_count,
      columns: t.columns.map((c) => ({
        name: c.name,
        data_type: c.data_type,
        min: c.profile?.min ?? null,
        max: c.profile?.max ?? null,
      })),
    })),
  );
  return { tables, loading: sources.isPending || details.some((d) => d.isPending) };
}

export function useSourcesInvalidation(projectId: string) {
  const client = useQueryClient();
  return (detail?: SourceDetail) => sourceChanged(client, projectId, detail);
}
