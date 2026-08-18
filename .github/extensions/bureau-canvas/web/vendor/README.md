# Vendored renderer modules

The canvas renders with React and React Flow. They are committed here rather
than fetched at run time.

## Why vendored

The panel edits `.bureau/`, and PR review of that config is the entire
authorization model (DESIGN.md §5). Pulling third-party JavaScript from a CDN
at render time would put unreviewed code in the path of the tool that edits the
authorization model, and would make the panel need network to draw at all —
which sits badly in a repository whose tests are offline by rule.

Committed means reviewed once, pinned by content, and working with no network.

## Contents

| File | Package | Version |
|---|---|---|
| `react.mjs` | `react` | 18.3.1 |
| `react-jsx-runtime.mjs` | `react/jsx-runtime` | 18.3.1 |
| `react-dom.mjs` | `react-dom` | 18.3.1 |
| `react-dom-client.mjs` | `react-dom/client` | 18.3.1 |
| `xyflow-react.mjs` | `@xyflow/react` | 12.3.5 |
| `xyflow-react.css` | `@xyflow/react/dist/style.css` | 12.3.5 |

Roughly 470 KB in total. `react-dom` and `react-dom/client` are both present
because the app imports `createRoot` from the latter and `@xyflow/react`
imports the former.

These are esm.sh bundles built for `es2022`, with React kept external so a
single copy is shared. They import bare specifiers (`react`, `react-dom`,
`react/jsx-runtime`), which the import map in `../index.html` resolves to these
files — so there are no absolute URLs inside them to rewrite.

## Updating

Refetch at the new version and update both the table above and the import map,
keeping the stylesheet in lockstep (a CSS `<link>` cannot use an import-map
alias).

```
https://esm.sh/react@<v>/es2022/react.bundle.mjs
https://esm.sh/react@<v>/es2022/jsx-runtime.bundle.mjs
https://esm.sh/react-dom@<v>/X-ZXJlYWN0/es2022/react-dom.bundle.mjs
https://esm.sh/react-dom@<v>/X-ZXJlYWN0/es2022/client.bundle.mjs
https://esm.sh/@xyflow/react@<v>/X-ZXJlYWN0LHJlYWN0LWRvbQ/es2022/react.bundle.mjs
https://esm.sh/@xyflow/react@<v>/dist/style.css
```

The `X-...` path segment is esm.sh's encoding of the `external` set; fetching
`https://esm.sh/<pkg>@<v>?external=react,react-dom&bundle&target=es2022`
returns a small stub that names the current one.

Then run the browser suite (`node ../../e2e/run.mjs`), which fails on any
console error and so catches a bad or partial fetch.
