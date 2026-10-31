# PostgreSQL 18 + Apache AGE (graph) + pgvector (embeddings).
# Versions verified 2026-10-31 against the PGDG trixie repo; see docs/adr/0003-version-pins.md.
# Apache AGE tags every final release "-rc0" (PG18/v1.8.0-rc0 is the GA release), hence the odd suffix.
FROM postgres:18.6-trixie

ARG AGE_VERSION=1.8.0~rc0-2.pgdg13+1
ARG PGVECTOR_VERSION=0.8.7-1.pgdg13+1

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      "postgresql-18-age=${AGE_VERSION}" \
      "postgresql-18-pgvector=${PGVECTOR_VERSION}" \
 && rm -rf /var/lib/apt/lists/* \
 # Expose AGE under $libdir/plugins so non-superuser roles can `LOAD '$libdir/plugins/age'`.
 # Postgres de-duplicates by inode, so this is the same library shared_preload_libraries loaded.
 && mkdir -p /usr/lib/postgresql/18/lib/plugins \
 && ln -s ../age.so /usr/lib/postgresql/18/lib/plugins/age.so
