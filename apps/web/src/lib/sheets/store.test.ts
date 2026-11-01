import type { Sheet } from "@lattice/shared-types";
import { beforeEach, describe, expect, it } from "vitest";

import { createSheetStore } from "./store";

const sheet: Sheet = {
  id: "s1",
  project_id: "p1",
  name: "Revenue Review",
  position: 1,
  filters: [],
  visuals: [],
  version: 3,
  updated_at: "2026-11-01T00:00:00Z",
  link_audience: "off",
};

describe("sheet store", () => {
  beforeEach(() => sessionStorage.clear());

  it("undoes and redoes edits as commands", () => {
    const store = createSheetStore("p1", sheet, true);
    const rename = (name: string) => store.getState().edit("Rename sheet", (d) => ({ ...d, name }));
    rename("A");
    rename("B");
    expect(store.getState().past.map((c) => c.label)).toEqual(["Rename sheet", "Rename sheet"]);
    store.getState().undo();
    expect(store.getState().doc.name).toBe("A");
    store.getState().undo();
    expect(store.getState().doc.name).toBe("Revenue Review");
    store.getState().redo();
    expect(store.getState().doc.name).toBe("A");
    rename("C");
    expect(store.getState().future).toEqual([]);
    expect(store.getState().saveState).toBe("dirty");
  });

  it("merges rapid edits with the same key into one command", () => {
    const store = createSheetStore("p1", sheet, true);
    for (const name of ["R", "Re", "Rev"]) {
      store.getState().edit("Rename sheet", (d) => ({ ...d, name }), "name");
    }
    expect(store.getState().past).toHaveLength(1);
    store.getState().undo();
    expect(store.getState().doc.name).toBe("Revenue Review");
  });

  it("keeps history across reloads only while the version matches", () => {
    const store = createSheetStore("p1", sheet, true);
    store.getState().edit("Rename sheet", (d) => ({ ...d, name: "X" }));
    expect(createSheetStore("p1", sheet, true).getState().past).toHaveLength(1);
    expect(createSheetStore("p1", { ...sheet, version: 4 }, true).getState().past).toHaveLength(0);
  });

  it("tracks drill paths and stamps interactions", () => {
    const store = createSheetStore("p1", sheet, false);
    store.getState().drillDown("v1", "EMEA");
    store.getState().drillDown("v1", "Outdoor");
    expect(store.getState().drill.v1).toEqual(["EMEA", "Outdoor"]);
    store.getState().drillTo("v1", 1);
    expect(store.getState().drill.v1).toEqual(["EMEA"]);
    expect(store.getState().interactionAt).toBeGreaterThan(0);
  });

  it("stays dirty when edited during a save", () => {
    const store = createSheetStore("p1", sheet, true);
    store.getState().edit("Rename sheet", (d) => ({ ...d, name: "A" }));
    store.getState().saving();
    store.getState().edit("Rename sheet", (d) => ({ ...d, name: "B" }));
    store.getState().saved(4);
    expect(store.getState()).toMatchObject({ version: 4, saveState: "dirty" });
  });
});
