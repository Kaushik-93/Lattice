import { create } from "zustand";

interface UiState {
  perfOverlayOpen: boolean;
  togglePerfOverlay: () => void;
  setPerfOverlayOpen: (open: boolean) => void;
}

/** Small client-only UI state. Large data (Arrow tables, phase 1+) never lives here. */
export const useUiStore = create<UiState>()((set) => ({
  perfOverlayOpen: false,
  togglePerfOverlay: () => set((s) => ({ perfOverlayOpen: !s.perfOverlayOpen })),
  setPerfOverlayOpen: (open) => set({ perfOverlayOpen: open }),
}));
