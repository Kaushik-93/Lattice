"use client";

import type {
  AuthProvidersResponse,
  CreateProjectRequest,
  LoginRequest,
  MeResponse,
  ProjectDetail,
  ProjectListResponse,
  ProjectSummary,
  SignupRequest,
} from "@lattice/shared-types";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { ApiError, api } from "@/lib/gateway";

export const queryKeys = {
  me: ["me"] as const,
  providers: ["auth", "providers"] as const,
  projects: ["projects"] as const,
  project: (id: string) => ["projects", id] as const,
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
