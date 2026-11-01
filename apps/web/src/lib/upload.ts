import type { ApiErrorBody, SourceDetail } from "@lattice/shared-types";

import { ApiError, GATEWAY_URL } from "@/lib/gateway";

export const MAX_UPLOAD_BYTES = 200 * 1024 * 1024;
export const ACCEPTED_FILES = ".csv,.tsv,.json,.ndjson,.jsonl,.parquet,.xlsx,.sqlite,.sqlite3,.db";

/**
 * Uploads a file as a new source. Uses XMLHttpRequest because fetch can't report upload
 * progress; `onProgress` receives 0..1 while bytes are sent, then the gateway loads and profiles.
 */
export function uploadSource(
  projectId: string,
  file: File,
  onProgress: (fraction: number) => void,
): Promise<SourceDetail> {
  return new Promise((resolve, reject) => {
    if (file.size > MAX_UPLOAD_BYTES) {
      reject(new ApiError(413, "too_large", "That file is larger than the 200 MB limit."));
      return;
    }
    const xhr = new XMLHttpRequest();
    xhr.open("POST", `${GATEWAY_URL}/api/projects/${encodeURIComponent(projectId)}/sources/upload`);
    xhr.withCredentials = true;
    xhr.upload.onprogress = (event) => {
      if (event.lengthComputable) onProgress(event.loaded / event.total);
    };
    xhr.onerror = () => reject(new ApiError(0, "network", "Upload failed: can't reach Lattice."));
    xhr.onload = () => {
      let body: unknown;
      try {
        body = JSON.parse(xhr.responseText);
      } catch {
        body = undefined;
      }
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve(body as SourceDetail);
        return;
      }
      const error = (body as ApiErrorBody | undefined)?.error;
      reject(
        new ApiError(
          xhr.status,
          error?.code ?? `http_${xhr.status}`,
          error?.message ?? `Upload failed (${xhr.status}).`,
        ),
      );
    };
    const form = new FormData();
    form.append("file", file);
    xhr.send(form);
  });
}
