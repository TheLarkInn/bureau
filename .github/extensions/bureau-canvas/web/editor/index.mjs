// Entry point for editor.html: loads the shared state, mounts the pipeline
// editor for the selected pipeline, and keeps the relation graph (Q16) one
// tab away. State still streams from the same server; only the surface is
// new.

import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import { PipelineEditor } from "./editor.mjs";
import { RelationGraph } from "./relation.mjs";
import { isReadOnly } from "../access-policy.mjs";

const h = React.createElement;

createRoot(document.querySelector("#editor-root")).render(h(EditorApp));
window.__bureauEditorMounted = true;
window.dispatchEvent(new Event("bureau-editor-mounted"));

async function backToAssignments() {
  await fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "back-to-config" }),
  });
  window.location.assign("./");
}

function EditorApp() {
  const [state, setState] = useState(null);
  const [tab, setTab] = useState("pipeline");
  const [dirty, setDirty] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const dirtyNow = useRef(false);
  const route = useRef({ seen: false, revision: null, selection: null, pinned: null });
  dirtyNow.current = dirty;
  const receive = (next) => {
    const current = route.current;
    const changed = current.seen && next.navigation?.revision
      && next.navigation.revision !== current.revision;
    current.seen = true;
    current.revision = next.navigation?.revision;
    if (changed) {
      if (!dirtyNow.current || window.confirm("Discard unsaved pipeline changes?")) {
        setLeaving(true);
        return;
      }
      current.pinned = current.selection;
    }
    current.selection = current.pinned ?? next.selectedPipeline;
    setState({ ...next, selectedPipeline: current.selection });
  };
  const navigate = (action) => {
    if (!dirty || window.confirm("Discard unsaved pipeline changes?")) {
      dirtyNow.current = false;
      setDirty(false);
      action();
    }
  };
  useEffect(() => {
    if (leaving) window.location.assign("./");
  }, [leaving]);

  useEffect(() => {
    let alive = true;
    // The SSE channel can deliver before this fetch resolves, and its payload
    // is the newer one. The fetch fills the surface only if nothing has
    // arrived yet, so a slow response cannot revert the editor.
    fetch("./state", { cache: "no-store" })
      .then((response) => response.json())
      .then((next) => { if (alive && !route.current.seen) receive(next); });
    const events = new EventSource("./events");
    // The same local-state channel index.html's `App` listens on. Nothing in
    // editor.html's own bundle dispatches it: this is the seam the state lab
    // and the matrix suite publish fixtures through, so both surfaces receive
    // a payload the same way without a test-only flag in production code.
    const localState = (event) => receive(event.detail);
    events.addEventListener("state", (event) => receive(JSON.parse(event.data)));
    window.addEventListener("bureau-state", localState);
    return () => {
      alive = false;
      events.close();
      window.removeEventListener("bureau-state", localState);
    };
  }, []);

  if (leaving) {
    return h("main", { className: "app-shell" }, h("p", { role: "status" }, "Opening requested view..."));
  }
  if (!state) {
    return h("main", { className: "app-shell" }, h("p", { className: "status" }, "Loading…"));
  }
  if (isReadOnly(state.access)) {
    return h("main", { className: "app-shell" },
      h("header", { className: "app-header" }, h("h1", {}, "Read-only pipeline inspection")),
      h("p", { id: "read-only-notice", className: "status", role: "status" }, state.access.reason),
      h("a", { className: "btn btn--small", href: "./" }, "Return to dashboard"));
  }
  const name = state.selectedPipeline?.name;
  const missing = !name || state.selectedPipeline?.missing;
  return h(
    "main",
    { className: "app-shell" },
    h(
      "header",
      { className: "app-header" },
      h(
        "div",
        { className: "editor-heading" },
        h("button", { type: "button", className: "btn btn--small", "data-testid": "editor-back", onClick: () => navigate(backToAssignments) }, "← Assignments"),
        h("div", {}, h("h1", {}, "Pipeline editor"), h("p", { className: "summary" }, name ?? state.dir)),
      ),
      h(
        "nav",
        { className: "editor-tabs", "aria-label": "Editor view" },
        h("button", {
          type: "button",
          className: `editor-tab${tab === "pipeline" ? " is-active" : ""}`,
          "aria-pressed": tab === "pipeline",
          "data-testid": "editor-tab-pipeline",
          onClick: () => setTab("pipeline"),
        }, "Pipeline"),
        h("button", {
          type: "button",
          className: `editor-tab${tab === "relations" ? " is-active" : ""}`,
          "aria-pressed": tab === "relations",
          "data-testid": "editor-tab-relations",
          onClick: () => setTab("relations"),
        }, "Relations"),
      ),
    ),
    h("section", { className: "editor-view", hidden: tab !== "pipeline" },
      missing
        ? h("p", { className: "status" }, name ? `No pipeline named \`${name}\` in this config.` : "Open a pipeline from the config view first.")
        : h(PipelineEditor, { state, name, onSaved: receive, onDirtyChange: setDirty })),
    h("section", { className: "editor-view", hidden: tab !== "relations" },
      h(RelationGraph, { relation: state.config?.relation })),
  );
}
