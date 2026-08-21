---
name: Bureau Canvas
description: Agent-ops graph console — a GitHub Primer-native drafting table for pipeline drawings
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
  node-card:
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

**Creative North Star: "The Drafting Table"**

Bureau Canvas treats pipelines as drawings under a lamp. The chrome is
sheet-metal neutral — a GitHub Primer-derived light surface that any
developer already knows how to read — so the graph itself carries all
expression. Precision tools, not decoration: pill badges, mono identifiers,
and color-coded kinds are the only personality, and they exist to make agent
workflows legible, not to be noticed.

The system is deliberately indistinguishable from Primer. Familiarity is the
feature: an operator who has used GitHub already knows every control,
weight, and border on this bench. Ambient shadow is allowed only on
persistent chrome (headers, panels, menus); resting content surfaces stay
flat.

**Key Characteristics:**

- Primer-native light palette with `color-scheme: light dark` declared
- Color-coded graph semantics: every node kind and outcome owns a hue
- Pill badges and mono IDs as the signature density markers
- Flat content, ambient chrome
- 4px-rem spacing scale (`--space-1` … `--space-5`)

## Colors

A light Primer palette where functional gray dominates and saturated hues
are reserved for graph semantics and outcomes.

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
states, explanatory text — renders in Bench Gray, never a tinted variant.

## Typography

**Display/Body Font:** system stack (-apple-system, BlinkMacSystemFont,
"Segoe UI", sans-serif)
**Label/Mono Font:** SFMono-Regular, Consolas, Liberation Mono

**Character:** invisible by design. The system stack reads as GitHub
instantly; mono appears only for identifiers, hashes, and machine truth.

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
Graph node cards hold a fixed 15rem width (`--card-width`) so drawings stay
uniform at any zoom.

At **56rem and below**, dense toolbars and two-column control rows collapse
to their compact layout. This is the one recorded responsive breakpoint for
both the config surface and pipeline editor.

## Elevation & Depth

Flat by default: content surfaces have no shadow. Ambient elevation is
permitted on persistent chrome only — the overflow menu lifts with a soft
structural shadow (0 10px 24px rgba(31,35,40,0.08)), and a deeper variant
(0 8px 24px rgb(0 0 0 / 28%)) marks overlay-level surfaces. Graph nodes
convey selection and hover through glow shadows owned by the flow library,
not through resting elevation.

### Shadow Vocabulary
- **Chrome lift** (`box-shadow: 0 10px 24px rgba(31,35,40,0.08)`): menus
  and persistent panels above the bench.
- **Overlay lift** (`box-shadow: 0 8px 24px rgb(0 0 0 / 28%)`): transient
  overlays that must read as above everything.

### Named Rules

**The Flat Bench Rule.** Drawings lie flat on the table. Shadows answer
state (hover, selection, open menu) or chrome — never styling.

## Shapes

Softly squared engineering forms: default corner is gently rounded
(0.625rem via `--radius`), inner controls step down (6px), and badges go
fully pill (999px). Hairline borders (1px, Hairline) define every boundary;
fills rarely do. The recurring silhouette is the rounded-rectangle node
card with a pill badge — drawn, not decorated.

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
- **Background:** Sheet White
- **Shadow Strategy:** Flat Bench — none at rest
- **Border:** 1px Hairline
- **Internal Padding:** 12px 16px on node cards; spacing scale elsewhere

### Inputs / Fields
- **Style:** 1px Hairline stroke, Sheet White background, 6px radius
- **Focus:** Primer Link Blue border/ring shift
- **Error:** Washed Red background with Outcome Failure Red text

### Navigation
- App header: flex row, space-between, hairline bottom border; title in
  Headline, metadata in Body Bench Gray. No tabs, no breadcrumbs — the
  graph is the navigation.

### Graph Node (signature)
- Fixed-width card (15rem) on the drafting surface: kind-colored pill badge
  top-left, mono handle/ID in Code, title in Title. Selected and hovered
  states glow via the flow library's shadow tokens. Every visual property
  of the node answers "what kind of step is this and what is it doing."

## Do's and Don'ts

### Do:
- **Do** reserve saturated hue for graph semantics: blue=assignment/data,
  purple=role/agent/observes, green=repo/success, amber=pipeline/decision/
  blocked.
- **Do** keep spacing on the 0.25–1.5rem scale and corners at 0.625rem
  (6px for inner controls, 999px for badges).
- **Do** render identifiers, hashes, and handles in 12px mono.
- **Do** let ambient shadow appear only on chrome: menus, persistent
  panels, overlays.

### Don't:
- **Don't** use color decoratively — a hue without a semantic is a defect.
- **Don't** add resting shadows to content cards; the bench is flat.
- **Don't** introduce font weights outside 400/700, or display faces
  outside the system stack.
- **Don't** wrap panels in nested cards; hairline borders divide, fills
  don't.
