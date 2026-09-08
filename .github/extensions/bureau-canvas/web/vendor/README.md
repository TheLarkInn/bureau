# Vendored renderer modules

React and React Flow are committed so the config editor runs offline with
reviewed code, not JavaScript fetched from a CDN at runtime.

| File | Package | Version |
|---|---|---|
| `react.mjs` | `react` | 18.3.1 |
| `react-jsx-runtime.mjs` | `react/jsx-runtime` | 18.3.1 |
| `react-dom.mjs` | `react-dom` | 18.3.1 |
| `react-dom-client.mjs` | `react-dom/client` | 18.3.1 |
| `xyflow-react.mjs` | `@xyflow/react` | 12.3.5 |
| `xyflow-react.css` | `@xyflow/react/dist/style.css` | 12.3.5 |

These esm.sh ES2022 bundles share one React copy through the import maps in
[`index.html`](../index.html) and [`editor.html`](../editor.html).
Keep both React DOM modules: the app and React Flow use different entry points.

## Updating

Replace matching bundles, update the versions above and both import maps, and
keep the stylesheet at the React Flow version. Preserve bundled license notices.

```
https://esm.sh/react@<v>/es2022/react.bundle.mjs
https://esm.sh/react@<v>/es2022/jsx-runtime.bundle.mjs
https://esm.sh/react-dom@<v>/X-ZXJlYWN0/es2022/react-dom.bundle.mjs
https://esm.sh/react-dom@<v>/X-ZXJlYWN0/es2022/client.bundle.mjs
https://esm.sh/@xyflow/react@<v>/X-ZXJlYWN0LHJlYWN0LWRvbQ/es2022/react.bundle.mjs
https://esm.sh/@xyflow/react@<v>/dist/style.css
```

`X-...` encodes external dependencies. To find its current value, fetch
`https://esm.sh/<pkg>@<v>?external=react,react-dom&bundle&target=es2022`
and follow the returned import.

Run the [browser tests](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/e2e/README.md)
after updating.
