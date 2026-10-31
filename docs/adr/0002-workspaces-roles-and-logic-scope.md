# ADR 0002 — Workspaces, roles and the scope of business logic

- Status: accepted (decided by the product owner, 2026-10-31)
- Context: the spec and the frontend design ([docs/design](../design/)) disagreed on two points
  that shape the data model from phase 0 onwards.

## 1. Tenancy: workspaces

The design has an organisation ("Acme Retail", "Create a workspace", "Anyone at Acme Retail with
the link"); the spec has only projects. **Decision:** a `workspace` is the tenant. Users belong to
workspaces (`workspace_members`), projects belong to exactly one workspace, and every session is
bound to one active workspace. Every project query is scoped by the caller's workspace, and a
project in another workspace answers 404 (not 403), so ids can't be probed.

## 2. Business-logic scope: organisation-wide + project-local

- Spec §1/§6: each project owns its business logic.
- Design screen 05: business logic is organisation-wide ("every project and every copilot answer
  is built on", "Publishing this version updates every project that uses it"), while screens 02/04
  still say each project has its own.

**Decision: layered.**

- **Workspace layer** (screen 05): shared entities, metrics, rules, dimensions, glossary. Defined
  in terms of entities, not physical tables. Changes show impact across every project that uses
  them before they are approved.
- **Project layer**: each project binds workspace entities to its own sources (sources and
  credentials stay per project, as screens 03/04 show) and may add project-local definitions,
  including local overrides that must be approved like anything else.
- The AGE graph spans both layers so impact analysis and lineage cross project boundaries.

Implemented in phase 3; phase 0 only adds the workspace table so the layering has an anchor.

## 3. Roles: Viewer / Editor / Admin per project, Admin / Member per workspace

The spec's roles (viewer / analyst / steward / admin) versus the design's (Viewer / Editor / Admin,
plus an "Owner" label and a workspace Admin). **Decision: the design's roles.**

| Project permission | Viewer | Editor | Admin |
|---|---|---|---|
| View all sheets | ✓ | ✓ | ✓ |
| Ask the copilot | ✓ | ✓ | ✓ |
| Create & edit sheets | — | ✓ | ✓ |
| Share sheets by link | — | ✓ | ✓ |
| Approve business logic (the spec's *steward*) | — | — | ✓ |
| Manage sources & members | — | — | ✓ |

- Editor ≈ the spec's *analyst*: can ask and **propose** logic; cannot approve it.
- "Owner" is the project's creator, who is an Admin; it is a label, not a separate role.
- Workspace roles: `admin` (manages workspace-level logic and membership) and `member`.
- The spec's rule still holds: approval is a separate, recorded act even when one person holds
  every role.
- The matrix lives in `lattice_security::rbac` and has a test that mirrors the design table.

Roles are stored as `text` with `CHECK` constraints rather than Postgres enums, so adding one is a
one-line migration.

## Consequences

- Approval rights for workspace-level logic need a rule (workspace admins, or named stewards per
  definition, which the design hints at with "Owner: Finance"); decided in phase 3.
- Sheet-level sharing (invite to one sheet, view links incl. anonymous links) comes from the
  design and is not in the spec; it lands with sheets (phase 2) and must respect row-level security
  for link viewers.
