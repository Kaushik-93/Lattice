-- Phase 0: tenancy, identity, sessions, projects and the audit log.
-- Roles follow docs/adr/0002-workspaces-and-roles.md: workspace admin|member, project viewer|editor|admin.
-- Roles are text + CHECK rather than Postgres enums so adding a role is a one-line migration.
-- Ids are UUIDv7 (Postgres 18 built-in): time-ordered, so B-tree inserts stay append-mostly.

CREATE TABLE workspaces (
    id          uuid PRIMARY KEY DEFAULT uuidv7(),
    name        text NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    slug        text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id             uuid PRIMARY KEY DEFAULT uuidv7(),
    email          text NOT NULL CHECK (email = lower(email) AND length(email) <= 254),
    display_name   text NOT NULL CHECK (length(display_name) BETWEEN 1 AND 80),
    -- NULL for accounts that only sign in through an identity provider.
    password_hash  text,
    created_at     timestamptz NOT NULL DEFAULT now(),
    last_login_at  timestamptz
);
CREATE UNIQUE INDEX users_email_key ON users (email);

-- Links an external identity-provider subject to a user.
CREATE TABLE user_identities (
    provider    text NOT NULL,
    subject     text NOT NULL,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    email       text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, subject)
);
CREATE INDEX user_identities_user_id_idx ON user_identities (user_id);

CREATE TABLE workspace_members (
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role          text NOT NULL CHECK (role IN ('admin', 'member')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_id_idx ON workspace_members (user_id);

-- Only a SHA-256 of the session token is stored; a database leak does not leak live sessions.
CREATE TABLE sessions (
    token_hash    bytea PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    created_at    timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz NOT NULL,
    user_agent    text
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE TABLE projects (
    id            uuid PRIMARY KEY DEFAULT uuidv7(),
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name          text NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    description   text NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    color         text NOT NULL CHECK (color ~ '^#[0-9A-F]{6}$'),
    created_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX projects_workspace_id_idx ON projects (workspace_id);

CREATE TABLE project_members (
    project_id  uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role        text NOT NULL CHECK (role IN ('viewer', 'editor', 'admin')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX project_members_user_id_idx ON project_members (user_id);

-- Append-only record of security-relevant actions (and, from phase 3, every BLL change
-- and every query run on behalf of the AI).
CREATE TABLE audit_log (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at            timestamptz NOT NULL DEFAULT now(),
    workspace_id  uuid REFERENCES workspaces (id) ON DELETE SET NULL,
    actor         text NOT NULL CHECK (actor ~ '^(user:[0-9a-f-]{36}|ai|system)$'),
    action        text NOT NULL,
    target        text,
    detail        jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX audit_log_workspace_at_idx ON audit_log (workspace_id, at DESC);
