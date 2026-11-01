"use client";

import type {
  KnowledgeGraph,
  ModelMetric,
  NewNote,
  NoteDetail,
  NoteEdit,
  ProjectModelResponse,
  QueueResponse,
  WorkspaceMembersResponse,
} from "@lattice/shared-types";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { ApiError, api } from "@/lib/gateway";

export const knowledgeKeys = {
  all: ["knowledge"] as const,
  graph: (project: string | null) => ["knowledge", "graph", project ?? "workspace"] as const,
  note: (id: string) => ["knowledge", "note", id] as const,
  queue: ["knowledge", "queue"] as const,
  // Under "knowledge" so every note change refreshes it.
  model: (project: string) => ["knowledge", "model", project] as const,
  members: ["workspace", "members"] as const,
};

const noRetryOn4xx = (count: number, error: Error) =>
  !(error instanceof ApiError && error.status >= 400 && error.status < 500) && count < 2;

export function useKnowledgeGraph(project: string | null) {
  return useQuery({
    queryKey: knowledgeKeys.graph(project),
    queryFn: ({ signal }) =>
      api<KnowledgeGraph>(`/api/knowledge/graph${project ? `?project=${project}` : ""}`, {
        signal,
      }),
    retry: noRetryOn4xx,
  });
}

export function useNote(id: string | null) {
  return useQuery({
    queryKey: knowledgeKeys.note(id ?? ""),
    queryFn: ({ signal }) => api<NoteDetail>(`/api/knowledge/notes/${id}`, { signal }),
    enabled: id !== null,
    retry: noRetryOn4xx,
  });
}

/** The semantic model a project sees: entities and their tables, metrics, rules. */
export function useProjectModel(project: string | null) {
  return useQuery({
    queryKey: knowledgeKeys.model(project ?? ""),
    queryFn: ({ signal }) =>
      api<ProjectModelResponse>(`/api/projects/${encodeURIComponent(project ?? "")}/model`, {
        signal,
      }),
    enabled: project !== null,
    retry: noRetryOn4xx,
    staleTime: 30_000,
  });
}

/** Metrics a visual on `tableId` can chart: they count the entity bound to that table. */
export function metricsFor(
  model: ProjectModelResponse | undefined,
  tableId: string,
): ModelMetric[] {
  if (!model) return [];
  const grains = new Set(
    model.entities.filter((e) => e.table?.table_id === tableId).map((e) => e.ident),
  );
  return model.metrics.filter((m) => m.grain !== null && grains.has(m.grain));
}

export function useReviewQueue() {
  return useQuery({
    queryKey: knowledgeKeys.queue,
    queryFn: ({ signal }) => api<QueueResponse>("/api/knowledge/queue", { signal }),
    select: (data) => data.items,
    retry: noRetryOn4xx,
  });
}

export function useWorkspaceMembers() {
  return useQuery({
    queryKey: knowledgeKeys.members,
    queryFn: ({ signal }) => api<WorkspaceMembersResponse>("/api/workspace/members", { signal }),
    select: (data) => data.members,
    staleTime: 5 * 60_000,
  });
}

/** Any write changes the note, the graph's colours and the queue. */
function changed(client: QueryClient, note: NoteDetail | null, id: string) {
  if (note) client.setQueryData(knowledgeKeys.note(id), note);
  else client.removeQueries({ queryKey: knowledgeKeys.note(id) });
  void client.invalidateQueries({ queryKey: ["knowledge", "graph"] });
  void client.invalidateQueries({ queryKey: knowledgeKeys.queue });
}

export function useCreateNote() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (note: NewNote) =>
      api<NoteDetail>("/api/knowledge/notes", { method: "POST", body: note }),
    onSuccess: (note) => changed(client, note, note.id),
  });
}

type Action =
  | { kind: "draft"; edit: NoteEdit }
  | { kind: "submit" }
  | { kind: "approve" }
  | { kind: "reject"; reason: string }
  | { kind: "discard" }
  | { kind: "revert"; version: number }
  | { kind: "deprecate"; deprecated: boolean }
  | { kind: "owner"; owner_id: string | null; team: string | null };

/** Every change to an existing note: drafts, the review steps, history, ownership. */
export function useNoteAction(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (action: Action): Promise<NoteDetail | null> => {
      const path = `/api/knowledge/notes/${id}`;
      switch (action.kind) {
        case "draft":
          return api<NoteDetail>(`${path}/draft`, { method: "PUT", body: action.edit });
        case "reject":
          return api<NoteDetail>(`${path}/reject`, {
            method: "POST",
            body: { reason: action.reason },
          });
        case "revert":
          return api<NoteDetail>(`${path}/revert`, {
            method: "POST",
            body: { version: action.version },
          });
        case "deprecate":
          return api<NoteDetail>(`${path}/deprecate`, {
            method: "POST",
            body: { deprecated: action.deprecated },
          });
        case "owner":
          return api<NoteDetail>(`${path}/owner`, {
            method: "PUT",
            body: { owner_id: action.owner_id, team: action.team },
          });
        case "discard":
          return api<NoteDetail | null>(`${path}/discard`, { method: "POST" });
        default:
          return api<NoteDetail>(`${path}/${action.kind}`, { method: "POST" });
      }
    },
    onSuccess: (note) => changed(client, note, id),
  });
}
