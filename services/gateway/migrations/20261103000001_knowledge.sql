-- Phase 3: the knowledge graph (docs/adr/0006). One graph per workspace: notes are shared
-- across the workspace or local to one project. A note's live content is its latest approved
-- revision; every change is a new revision moving draft → pending → approved (or rejected).

CREATE TABLE knowledge_notes (
    id            uuid PRIMARY KEY DEFAULT uuidv7(),
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- NULL: shared across the workspace. Otherwise visible only in that project.
    project_id    uuid REFERENCES projects (id) ON DELETE CASCADE,
    kind          text NOT NULL
                  CHECK (kind IN ('entity', 'metric', 'dimension', 'rule', 'relationship', 'term', 'fact')),
    -- The owner approves changes to a shared note (with workspace admins); the team is a label
    -- ("Finance") shown beside them.
    owner_id      uuid REFERENCES users (id) ON DELETE SET NULL,
    owner_team    text CHECK (length(owner_team) <= 60),
    deprecated_at timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX knowledge_notes_workspace_idx ON knowledge_notes (workspace_id, project_id);

CREATE TABLE knowledge_revisions (
    id            uuid PRIMARY KEY DEFAULT uuidv7(),
    note_id       uuid NOT NULL REFERENCES knowledge_notes (id) ON DELETE CASCADE,
    version       int NOT NULL CHECK (version >= 1),
    title         text NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    -- What links resolve against: the title lowercased with whitespace collapsed.
    title_key     text NOT NULL,
    body          text NOT NULL DEFAULT '' CHECK (length(body) <= 20000),
    -- Kind-specific structure (e.g. a metric's formula), compiled from phase 3.2 on.
    definition    jsonb,
    status        text NOT NULL
                  CHECK (status IN ('draft', 'pending', 'approved', 'rejected', 'superseded')),
    author        text NOT NULL CHECK (author ~ '^(user:[0-9a-f-]{36}|ai|system)$'),
    -- Where the knowledge came from: {source: manual|profile|sheet|conversation, evidence, confidence}.
    provenance    jsonb NOT NULL DEFAULT '{}'::jsonb,
    change_note   text CHECK (length(change_note) <= 500),
    reviewed_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    reviewed_at   timestamptz,
    -- Required when rejecting; kept and fed back to the AI.
    review_reason text CHECK (length(review_reason) <= 1000),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (note_id, version)
);
-- At most one open change per note, and at most one live version.
CREATE UNIQUE INDEX knowledge_revisions_open_key
    ON knowledge_revisions (note_id) WHERE status IN ('draft', 'pending');
CREATE UNIQUE INDEX knowledge_revisions_live_key
    ON knowledge_revisions (note_id) WHERE status = 'approved';
CREATE INDEX knowledge_revisions_status_idx ON knowledge_revisions (status);
CREATE INDEX knowledge_revisions_title_key_idx ON knowledge_revisions (title_key);

-- [[Links]] written in a revision, by the target's title key. Resolved at read time, so a link
-- to a note that doesn't exist yet shows as unresolved, as in Obsidian.
CREATE TABLE knowledge_links (
    revision_id  uuid NOT NULL REFERENCES knowledge_revisions (id) ON DELETE CASCADE,
    target_key   text NOT NULL,
    -- The title as written in the link, for showing unresolved targets.
    target_title text NOT NULL,
    PRIMARY KEY (revision_id, target_key)
);
CREATE INDEX knowledge_links_target_idx ON knowledge_links (target_key);
