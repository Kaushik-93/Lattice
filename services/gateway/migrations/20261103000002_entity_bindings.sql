-- Phase 3: which table an entity is in each project (docs/adr/0002, 0006). Shared entities are
-- defined in terms of the business ("Order"), not tables; each project binds them to its own.
CREATE TABLE entity_bindings (
    project_id  uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    note_id     uuid NOT NULL REFERENCES knowledge_notes (id) ON DELETE CASCADE,
    table_id    uuid NOT NULL REFERENCES source_tables (id) ON DELETE CASCADE,
    bound_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    bound_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, note_id)
);
