# ADR 0007 — Manual visual editing

- Status: accepted
- Date: 2026-11-01

## Context

The AI (phase 4) will propose sheets and visuals, but not every change will go through it: people
expect to shape a chart directly, as in other BI tools. The product owner asked for that
explicitly and chose the interaction (2026-11-01). Design screen 06 shows the right-hand panel as
the copilot, and as the visual editor in edit mode; it doesn't detail the editing tools, so these
decisions fill that gap.

## Decisions

1. **One global edit mode.** A toggle on the sheet header turns editing on for the session: it
   stays on across sheets and reloads until turned off. While it is on, clicking a visual selects
   it and the drawer shows it; clicking the canvas shows sheet settings. While it is off, a click
   cross-filters and a double-click drills, as before. Only editors see the toggle.
2. **The drawer has Data, Format and Filters tabs**, with a searchable field list on the Data tab:
   governed metrics first (ADR 0006), then the table's columns, marked "not governed". Fields drag
   into the visual's slots (axis, legend, values, drill) or are added with a button, so the same
   actions work from the keyboard.
3. **Format** covers series colours, data labels, legend position, axis titles, range and log
   scale, gridlines, and number formatting (decimals, prefix, suffix). It is display-only: stored in
   the spec's options and applied by the browser, so it never changes a query or its cache key.
4. **Filters** are chart-level filters (the spec's existing `filters`, which apply to that visual
   only) and **Top N**: the top or bottom N categories by a measure. Top N ranks categories, not
   rows, so it holds with a split series: the compiled query is wrapped to keep the categories
   whose total ranks within N. The same wrapper serves raw and governed visuals.
5. **Conditional formatting** (colour scales, data bars, threshold rules) on tables, pivots and
   KPIs, and **reference lines** (a constant, or the average, minimum or maximum of a measure) and a
   **KPI target**, are display options too.
6. **Everything the AI produces stays editable with these tools.** There is no AI-only setting.

Validation of every new option (sizes, colours, counts) lives with the sheet's other limits in the
`sheets` crate, so a saved spec is always one the browser can render.
