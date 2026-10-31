# Frontend design

`Lattice — Agentic BI.html` is the source of truth for the UI. Open it in a browser: it is a
self-unpacking bundle of eight 1440-px-wide screens.

| # | Screen | Implemented |
|---|---|---|
| 01 | Sign in | Phase 0 (`/login`, plus `/signup` in the same style) |
| 02 | All projects | Phase 0 (`/projects`, `/projects?filter=shared`) |
| 03 | Project overview | Phase 0 shell (`/projects/[id]`): header, members; sheets, sources and the agent digest show empty states until their phases |
| 04 | Project sources | Phase 1 |
| 05 | Business logic (organization-wide) | Phase 3 (`/logic` is a placeholder) |
| 06 | Sheet + AI copilot | Phases 2 and 4 |
| 07 | Share project or sheet | Phase 2 |
| 08 | Shared sheet (view access) | Phase 2 |

## Tokens

Defined once in `apps/web/src/app/globals.css` and mapped onto shadcn/ui's variables.

| Token | Value | Use |
|---|---|---|
| primary | `#4338CA` (hover `#3730A3`, link hover `#312E81`) | buttons, links, active states |
| background | `#F6F7F9` | app canvas |
| card | `#FFFFFF`, border `#E4E7EC`, radius 14px | cards and panels |
| input | border `#D0D5DD`, radius 10px, height 42–46px | controls |
| foreground / muted / subtle | `#101828` / `#556070` / `#5F6B7A` | text |
| accent | `#EEF0FF` on `#3730A3` | selected nav item |
| success / warning | `#E6F4F1` on `#0F5F58` / `#FEF3E2` on `#8A3B0B` | status pills |
| brand panel | `#0F1424`, `#171D31`, `#262E47` | sign-in story panel |
| fonts | Geist, Geist Mono | self-hosted via `next/font` |

Deliberate departures are listed in [ADR 0003](../adr/0003-version-pins-and-deviations.md) (SSO
label, sign-in footer copy). Product decisions the design forced (workspace-wide business logic,
the Viewer/Editor/Admin roles) are in [ADR 0002](../adr/0002-workspaces-roles-and-logic-scope.md).
