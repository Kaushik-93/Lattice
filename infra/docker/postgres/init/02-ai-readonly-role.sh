#!/usr/bin/env bash
# Read-only role for the AI service. It may only ever read the embeddings table
# (granted by the gateway migration that creates it, in phase 4). Everything else
# the AI needs goes through gateway tools so RBAC, RLS and auditing apply.
set -euo pipefail

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v ai_password="${LATTICE_AI_DB_PASSWORD:-lattice_ai}" \
  -v db="$POSTGRES_DB" <<'SQL'
CREATE ROLE lattice_ai_ro LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT
  PASSWORD :'ai_password';
REVOKE ALL ON DATABASE :"db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO lattice_ai_ro;
SQL
