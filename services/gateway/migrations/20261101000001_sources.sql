-- Phase 1: data sources and their catalog (docs/adr/0004).
-- A source belongs to one project. Its tables keep stable ids across refreshes (upserted on
-- (source, schema, table)), so sheets can reference them from phase 2 on.

CREATE TABLE sources (
    id             uuid PRIMARY KEY DEFAULT uuidv7(),
    project_id     uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    kind           text NOT NULL CHECK (kind IN ('file', 'sqlite', 'postgres', 'mysql', 'demo')),
    name           text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    -- Non-secret settings (file name and format, or host/port/database/user).
    config         jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- AES-256-GCM(nonce || ciphertext) of the credentials, bound to this row's id.
    secret         bytea,
    status         text NOT NULL DEFAULT 'syncing' CHECK (status IN ('syncing', 'ready', 'error')),
    status_detail  text,
    -- Bumped whenever the data may have changed; part of every result-cache key.
    data_version   bigint NOT NULL DEFAULT 1,
    created_by     uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    synced_at      timestamptz
);
CREATE INDEX sources_project_id_idx ON sources (project_id);

CREATE TABLE source_tables (
    id           uuid PRIMARY KEY DEFAULT uuidv7(),
    source_id    uuid NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    -- Schema inside the DuckDB catalog that holds the table: the extract schema for extracts,
    -- the remote schema for live sources.
    schema_name  text NOT NULL,
    table_name   text NOT NULL,
    mode         text NOT NULL CHECK (mode IN ('live', 'extract')),
    row_count    bigint,
    profiled_at  timestamptz,
    UNIQUE (source_id, schema_name, table_name)
);

CREATE TABLE source_columns (
    table_id   uuid NOT NULL REFERENCES source_tables (id) ON DELETE CASCADE,
    ordinal    int NOT NULL,
    name       text NOT NULL,
    data_type  text NOT NULL,
    nullable   boolean NOT NULL DEFAULT true,
    -- {min, max, approx_distinct, null_fraction, avg, samples}
    profile    jsonb,
    PRIMARY KEY (table_id, ordinal)
);
