"use client";

import type {
  CreateSheetRequest,
  SaveSheet,
  SaveSheetResponse,
  SharedSheetView,
  SharingUpdate,
  Sheet,
  SheetListResponse,
  SheetSharing,
  SheetSummary,
} from "@lattice/shared-types";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { ApiError, api } from "@/lib/gateway";

export const sheetKeys = {
  list: (projectId: string) => ["projects", projectId, "sheets"] as const,
  sheet: (projectId: string, sheetId: string) =>
    ["projects", projectId, "sheets", sheetId] as const,
  sharing: (projectId: string, sheetId: string) =>
    ["projects", projectId, "sheets", sheetId, "sharing"] as const,
  shared: (token: string) => ["shared", token] as const,
};

const noRetryOn4xx = (count: number, error: Error) =>
  !(error instanceof ApiError && error.status >= 400 && error.status < 500) && count < 2;

export function useSheets(projectId: string) {
  return useQuery({
    queryKey: sheetKeys.list(projectId),
    queryFn: ({ signal }) =>
      api<SheetListResponse>(`/api/projects/${projectId}/sheets`, { signal }),
    select: (data) => data.sheets,
    retry: noRetryOn4xx,
  });
}

function fetchSheet(projectId: string, sheetId: string, signal?: AbortSignal) {
  return api<Sheet>(`/api/projects/${projectId}/sheets/${sheetId}`, { signal });
}

export function useSheet(projectId: string, sheetId: string) {
  return useQuery({
    queryKey: sheetKeys.sheet(projectId, sheetId),
    queryFn: ({ signal }) => fetchSheet(projectId, sheetId, signal),
    retry: noRetryOn4xx,
    // The editor owns the live copy; refetching under it would discard nothing but waste work.
    staleTime: Infinity,
  });
}

/** Loads neighbouring sheets in the background so switching tabs is instant (spec §3 rule 5). */
export function prefetchSheet(client: QueryClient, projectId: string, sheetId: string): void {
  void client.prefetchQuery({
    queryKey: sheetKeys.sheet(projectId, sheetId),
    queryFn: ({ signal }) => fetchSheet(projectId, sheetId, signal),
    staleTime: Infinity,
  });
}

export function useCreateSheet(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (req: CreateSheetRequest) =>
      api<Sheet>(`/api/projects/${projectId}/sheets`, { method: "POST", body: req }),
    onSuccess: (sheet) => {
      client.setQueryData(sheetKeys.sheet(projectId, sheet.id), sheet);
      void client.invalidateQueries({ queryKey: sheetKeys.list(projectId), exact: true });
    },
  });
}

export function saveSheet(projectId: string, sheetId: string, body: SaveSheet) {
  return api<SaveSheetResponse>(`/api/projects/${projectId}/sheets/${sheetId}`, {
    method: "PUT",
    body,
  });
}

/** Keeps the tab list (names, visual counts) in step after an autosave. */
export function sheetSaved(
  client: QueryClient,
  projectId: string,
  sheetId: string,
  update: Pick<SheetSummary, "name"> & { visual_count: number },
): void {
  client.setQueryData<SheetListResponse>(sheetKeys.list(projectId), (data) =>
    data
      ? {
          sheets: data.sheets.map((s) =>
            s.id === sheetId ? { ...s, name: update.name, visual_count: update.visual_count } : s,
          ),
        }
      : data,
  );
}

export function useDeleteSheet(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (sheetId: string) =>
      api<void>(`/api/projects/${projectId}/sheets/${sheetId}`, { method: "DELETE" }),
    onSuccess: (_, sheetId) => {
      client.removeQueries({ queryKey: sheetKeys.sheet(projectId, sheetId) });
      void client.invalidateQueries({ queryKey: sheetKeys.list(projectId), exact: true });
    },
  });
}

export function useReorderSheets(projectId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) =>
      api<void>(`/api/projects/${projectId}/sheets/order`, { method: "POST", body: { ids } }),
    onMutate: (ids) => {
      client.setQueryData<SheetListResponse>(sheetKeys.list(projectId), (data) =>
        data
          ? {
              sheets: ids
                .map((id) => data.sheets.find((s) => s.id === id))
                .filter((s): s is SheetSummary => s !== undefined),
            }
          : data,
      );
    },
    onSettled: () =>
      void client.invalidateQueries({ queryKey: sheetKeys.list(projectId), exact: true }),
  });
}

export function useSharing(projectId: string, sheetId: string | null) {
  return useQuery({
    queryKey: sheetKeys.sharing(projectId, sheetId ?? ""),
    queryFn: ({ signal }) =>
      api<SheetSharing>(`/api/projects/${projectId}/sheets/${sheetId}/sharing`, { signal }),
    enabled: sheetId !== null,
    retry: noRetryOn4xx,
  });
}

function sharingChanged(client: QueryClient, projectId: string, sheetId: string, s: SheetSharing) {
  client.setQueryData(sheetKeys.sharing(projectId, sheetId), s);
  client.setQueryData<SheetListResponse>(sheetKeys.list(projectId), (data) =>
    data
      ? {
          sheets: data.sheets.map((x) =>
            x.id === sheetId ? { ...x, link_audience: s.audience } : x,
          ),
        }
      : data,
  );
}

export function useUpdateSharing(projectId: string, sheetId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (update: SharingUpdate) =>
      api<SheetSharing>(`/api/projects/${projectId}/sheets/${sheetId}/sharing`, {
        method: "PUT",
        body: update,
      }),
    onSuccess: (s) => sharingChanged(client, projectId, sheetId, s),
  });
}

export function useInviteToSheet(projectId: string, sheetId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (email: string) =>
      api<SheetSharing>(`/api/projects/${projectId}/sheets/${sheetId}/members`, {
        method: "POST",
        body: { email },
      }),
    onSuccess: (s) => sharingChanged(client, projectId, sheetId, s),
  });
}

export function useRemoveFromSheet(projectId: string, sheetId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (userId: string) =>
      api<SheetSharing>(`/api/projects/${projectId}/sheets/${sheetId}/members/${userId}`, {
        method: "DELETE",
      }),
    onSuccess: (s) => sharingChanged(client, projectId, sheetId, s),
  });
}

export function useSharedSheet(token: string) {
  return useQuery({
    queryKey: sheetKeys.shared(token),
    queryFn: ({ signal }) => api<SharedSheetView>(`/api/shared/${token}`, { signal }),
    retry: noRetryOn4xx,
    staleTime: Infinity,
  });
}
