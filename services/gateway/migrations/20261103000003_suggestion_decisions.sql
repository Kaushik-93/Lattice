-- Phase 3: what people decided about knowledge suggested from data profiles (docs/adr/0006).
-- Suggestions themselves are worked out from the catalog when asked for, so they stay current;
-- only the decisions are kept.
CREATE TABLE knowledge_suggestion_decisions (
    project_id  uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    key         text NOT NULL CHECK (length(key) <= 200),
    decision    text NOT NULL CHECK (decision IN ('accepted', 'dismissed')),
    note_id     uuid REFERENCES knowledge_notes (id) ON DELETE SET NULL,
    decided_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    decided_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, key)
);
