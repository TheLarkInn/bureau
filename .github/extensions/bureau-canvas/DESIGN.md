---
name: Bureau Canvas
description: Bureau pipeline console — native config controls with a scoped charcoal graph workbench
colors:
  fg-default: "#1f2328"
  fg-muted: "#656d76"
  bg-default: "#ffffff"
  border-default: "#d0d7de"
  accent-blue: "#0969da"
  kind-role-purple: "#8250df"
  kind-repo-green: "#1a7f37"
  kind-pipeline-amber: "#9a6700"
  outcome-failure-red: "#cf222e"
  finding-validation-bg: "#ffebe9"
  finding-advisory-bg: "#fff8c5"
  surface-subtle: "#f6f8fa"
  surface-hover: "#eaeef2"
  surface-active: "#d0d7de"
typography:
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif"
    fontSize: "1.25rem"
    fontWeight: 700
    lineHeight: "1.5rem"
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif"
    fontSize: "1rem"
    fontWeight: 700
    lineHeight: "1.25rem"
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif"
    fontSize: "0.78rem"
    fontWeight: 700
    lineHeight: "1rem"
    letterSpacing: "0.08em"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif"
    fontSize: "14px"
    lineHeight: "20px"
  code:
    fontFamily: "\"SFMono-Regular\", Consolas, \"Liberation Mono\", monospace"
    fontSize: "12px"
rounded:
  sm: "6px"
  md: "0.625rem"
  pill: "999px"
spacing:
  xs: "0.25rem"
  sm: "0.5rem"
  md: "0.75rem"
  lg: "1rem"
  xl: "1.5rem"
components:
  config-card:
    backgroundColor: "{colors.bg-default}"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "12px 16px"
  badge-kind:
    backgroundColor: "{colors.kind-role-purple}"
    textColor: "{colors.bg-default}"
    rounded: "{rounded.pill}"
    padding: "2px 10px"
---

# Design System: Bureau Canvas

## Overview

**Creative North Star: "The Drafting Table", with a graph workbench**

Configuration and transition tables remain familiar, host-theme-native
surfaces. The light Primer values in the frontmatter are their fallback
palette, not a requirement to force a white page in a dark host.

Graph mode is the deliberate exception: a charcoal workbench makes step
names, execution state, and handoffs the first things to read. Its restrained
depth, name-first cards, and visible navigation tools draw on
[GraphCode](https://github.com/scgopi/GraphCode/tree/a5f4e1e2d43a2c577ede1035377faf439897bfa9)
as a source and screenshot reference. Bureau copies no GraphCode code or assets.
At that revision, the [app and daemon](https://github.com/scgopi/GraphCode/blob/a5f4e1e2d43a2c577ede1035377faf439897bfa9/LICENSE)
are FSL-1.1-MIT, not yet MIT; [GraphcodeKit](https://github.com/scgopi/GraphCode/blob/a5f4e1e2d43a2c577ede1035377faf439897bfa9/GraphcodeKit/LICENSE)
and the [CLI](https://github.com/scgopi/GraphCode/blob/a5f4e1e2d43a2c577ede1035377faf439897bfa9/graphcode-cli/LICENSE)
are MIT.

The exception is scoped to the graph and its graph-mode chrome. Relation
graph interiors use the same dark surface; opening a relation disclosure
does not darken the surrounding configuration form. Returning to Transitions
returns to native controls. Transitions remains the default authoring view.

**Key Characteristics:**

- Host-theme-native config and transition surfaces; scoped charcoal graphs
- Color-coded graph semantics: every node kind and outcome owns a hue
- Name-first graph cards, left type stripes, mono context, explicit state badges
- Flat native content; restrained depth on graph cards and chrome
- 4px-rem spacing scale (`--space-1` … `--space-5`)

## Colors

A native Primer fallback palette where functional gray dominates and
saturated hues are reserved for graph semantics and outcomes. Graph-scoped
overrides use charcoal surfaces, light text, quiet borders, and readable
semantic accents; do not reuse a light-surface ink color without checking
contrast on its dark background.

### Primary
- **Primer Link Blue** (#0969da): assignments, deterministic steps, data
  relations, and every interactive/link affordance. The one true accent.

### Secondary
- **Role Purple** (#8250df): roles, agent steps, and "observes" relations —
  the hue of delegated authority.

### Tertiary
- **Ledger Amber** (#9a6700): pipelines, decision steps, blocked outcomes,
  advisory findings. Signals judgment required, never decoration.
- **Worktree Green** (#1a7f37): repos, concurrent steps, success outcomes.

### Neutral
- **Ink Default** (#1f2328): primary text.
- **Bench Gray** (#656d76): muted text, work sources, terminal nodes,
  no-work outcomes — the color of context rather than action.
- **Hairline** (#d0d7de): borders and dividers.
- **Sheet White** (#ffffff): default background.
- **Control Surface** (#f6f8fa / #eaeef2 / #d0d7de): quiet control rest,
  hover, and active fills drawn from the neutral Primer ramp.
- **Washed Red** (#ffebe9) / **Washed Amber** (#fff8c5): finding
  backgrounds for validation errors and advisories.

### Named Rules

**The Hue-Has-Meaning Rule.** Saturated color is never ornamental: blue,
purple, green, and amber each name a graph kind, outcome, or relation. If a
hue carries no semantic, it does not ship.

**The Bench Gray Rule.** Anything contextual — work sources, terminal
identifiers, explanatory text — renders in neutral muted ink appropriate to
its surface. Explicit outcome and state badges retain their semantic color.

**The Scoped Workbench Rule.** Dark graph styling must not escape into config
forms or transition tables. Color does not replace a state word, an outcome
caption, a finding, or keyboard focus. Pending is not a request for attention.

## Typography

**Display/Body Font:** system stack (-apple-system, BlinkMacSystemFont,
"Segoe UI", sans-serif)
**Label/Mono Font:** SFMono-Regular, Consolas, Liberation Mono

**Character:** invisible by design. The system stack remains familiar; mono
appears for identifiers, hashes, commands, roles, and machine context.

### Hierarchy
- **Headline** (700, 1.25rem/1.5rem): page title in the app header; the
  only 1.25rem text on the surface.
- **Title** (700, 1rem/1.25rem): section and panel titles.
- **Body** (400, 14px/20px): all operational reading text.
- **Label** (700, 0.78rem/1rem, 0.08em, uppercase, Bench Gray): group and
  rail headings — small caps that organize without shouting.
- **Code** (400, 12px): run IDs, hashes, config keys, node handles.

### Named Rules

**The One Loud Voice Rule.** Weight 700 is the only loud voice; it belongs
to headlines, titles, and labels. Body text never bolds for emphasis — it
restructures instead.

## Layout

A single-column flex app shell (`app-shell`) because the draft bar appears
and disappears; fixed grid rows stretch whichever child lands in them. The
header is a hairline-separated bar with 1rem/1.5rem padding. Spacing moves
on a 4px-rem scale: 0.25 / 0.5 / 0.75 / 1 / 1.5rem (`--space-1` …
`--space-5`), and nothing lands off it. The graph canvas owns the remaining
viewport; rails and panels hug its edges rather than floating over it.
Graph step cards share a consistent width so names, context, and states are
comparable. The viewer and editor use the same `layoutPipeline` left-to-right
placement rather than presenting one pipeline as two different drawings.
Saved `layout.json` coordinates remain authoritative; live and replay state
decorations do not move the steps.

Find steps/nodes, the attention queue, and camera controls sit in graph-local
screen space. Ctrl/Cmd+K opens and focuses search on a visible mounted graph;
hidden editor/relation surfaces do not capture the shortcut. The visible
trigger exposes the shortcut through its title and `aria-keyshortcuts`.
Search covers name, type, context, and state; Enter selects and
centers the first match, results are tabbable, and Escape closes search and
restores its trigger. Review next selects failure, blocked, paused, or findings
without treating ordinary pending work as actionable. A visible zoom percentage
also offers Actual size; Fit and the minimap provide overview and recovery.
The shared `initialGraphViewport` opens at 80–100% rather than shrinking
cards until everything fits. Oversized drawings align left to keep the entry
readable; explicit Fit may zoom out to 20%. Initial framing runs once, after
React Flow's internal nodes are measured and the surface is visible, including
a relation graph revealed after mounting. Read-only controlled node props
need not carry those measurements. Later observation preserves the user's camera.

At **56rem and below**, dense toolbars and two-column control rows collapse
to their compact layout. This is the one recorded responsive breakpoint for
both the config surface and pipeline editor.

## Elevation & Depth

Native content is flat by default. Ambient elevation is
permitted on persistent chrome — the overflow menu lifts with a soft
structural shadow (0 10px 24px rgba(31,35,40,0.08)), and a deeper variant
(0 8px 24px rgb(0 0 0 / 28%)) marks overlay-level surfaces. Within the
charcoal graph only, a restrained card lift and tonal surface separate nodes
from connections. Selection and attention must remain distinct from this
resting depth, with borders, state words, and visible focus.

### Shadow Vocabulary
- **Chrome lift** (`box-shadow: 0 10px 24px rgba(31,35,40,0.08)`): menus
  and persistent panels above the bench.
- **Overlay lift** (`box-shadow: 0 8px 24px rgb(0 0 0 / 28%)`): transient
  overlays that must read as above everything.

### Named Rules

**The Flat Bench Rule.** Native content remains flat. The scoped graph-card
exception uses depth to separate readable cards from connections, not to
decorate every container or create a stack of competing panels.

## Shapes

Softly squared engineering forms: default corner is gently rounded
(0.625rem via `--radius`), inner controls step down (6px), and badges go
fully pill (999px). Hairline borders (1px, Hairline) define every boundary;
fills rarely do outside graph mode. The graph silhouette is a rounded
rectangle with a left type stripe and a separate state badge.

## Components

### Buttons
- **Shape:** gently squared (6px or `--radius` 0.625rem)
- **Primary:** Primer Link Blue background, Sheet White text, compact
  padding; inherits font
- **Hover / Focus:** state shadow or border shift; no bounce easing

### Chips / Badges
- **Style:** pill (999px), kind-colored background (Role Purple, Ledger
  Amber, Worktree Green, Primer Link Blue) with Sheet White text, compact
  padding (2px 10px)
- **State:** static semantic markers — they identify, they do not toggle

### Cards / Containers
- **Corner Style:** gently rounded (0.625rem)
- **Background:** host-native for config; charcoal tonal surface for graph cards
- **Shadow Strategy:** flat native content; restrained graph-card lift
- **Border:** 1px surface-appropriate hairline; visible selected/attention treatment
- **Internal Padding:** 12px 16px on node cards; spacing scale elsewhere

### Inputs / Fields
- **Style:** 1px Hairline stroke, Sheet White background, 6px radius
- **Focus:** Primer Link Blue border/ring shift
- **Error:** Washed Red background with Outcome Failure Red text

### Navigation
- App header: flex row, space-between, hairline bottom border; title in
  Headline, metadata in Body muted ink. Existing Design/Live/Replay and
  Transitions/Graph controls remain; graph search supplements navigation.

### Graph Node (signature)
- Name first, with a kind-colored stripe on the left, mono command/role
  context beneath, and an explicit state badge. Type remains available as
  text rather than depending on the stripe alone. Design and Pending are
  different from Running; an unrun step must not imply success.
- Graph step command/context previews show at most three lines, so long
  commands cannot cover the next card in the same layer. The full command
  remains in the viewer's selected-step inspector, the editor's editable
  command field, and search context. Terminal copy and findings are not truncated.
- Forward handoffs use quiet curves and semantic outcome captions. Retry
  routes and data dependencies remain visible; calm styling must not erase
  an exception path. Viewer and editor terminal-exit captions share a separate
  label rail rather than following long exit curves across cards. Each curve
  passes through its own caption so repeated outcomes remain traceable;
  Publish and other terminal titles must remain readable. Existing editor ports and the
  transition table retain
  accessible rewiring rather than depending on tiny hover-only targets.
- Selecting a viewer step exposes its configuration and handoffs in the
  side panel. Live/replay logs remain below the graph. This inspector does
  not replace the editor's full draft, save, validate, and revert flow.

## Do's and Don'ts

### Do:
- **Do** reserve saturated hue for graph semantics: blue=assignment/data,
  purple=role/agent/observes, green=repo/success, amber=pipeline/decision/
  blocked.
- **Do** keep spacing on the 0.25–1.5rem scale and corners at 0.625rem
  (6px for inner controls, 999px for badges).
- **Do** render identifiers, hashes, and handles in 12px mono.
- **Do** limit resting card depth to the scoped graph workbench; keep native
  forms flat and preserve readable text at Actual size.

### Don't:
- **Don't** use color decoratively — a hue without a semantic is a defect.
- **Don't** spread the graph's dark palette or card shadows to native forms.
- **Don't** equate pending with needs-attention, hide outcomes behind color,
  or reduce essential actions to hover-only affordances.
- **Don't** introduce font weights outside 400/700, or display faces
  outside the system stack.
- **Don't** wrap panels in nested cards; hairline borders divide, fills
  don't.
