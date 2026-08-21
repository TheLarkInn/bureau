// Entry point for editor.html: loads the shared state, mounts the pipeline
// editor for the selected pipeline, and keeps the relation graph (Q16) one
// tab away. State still streams from the same server; only the surface is
// new.

import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { PipelineEditor } from "./editor.mjs";
import { RelationGraph } from "./relation.mjs";

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
  const navigate = (action) => {
    if (!dirty || window.confirm("Discard unsaved pipeline changes?")) {
      setDirty(false);
      action();
    }
  };

  useEffect(() => {
    let alive = true;
    fetch("./state", { cache: "no-store" })
      .then((response) => response.json())
      .then((next) => alive && setState(next));
    const events = new EventSource("./events");
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    return () => {
      alive = false;
      events.close();
    };
  }, []);

  if (!state) {
    return h("main", { className: "app-shell" }, h("p", { className: "status" }, "Loading…"));
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
        h("button", { type: "button", className: "btn btn--small", onClick: () => navigate(backToAssignments) }, "← Assignments"),
        h("div", {}, h("h1", {}, "Pipeline editor"), h("p", { className: "summary" }, name ?? state.dir)),
      ),
      h(
        "nav",
        { className: "editor-tabs", "aria-label": "Editor view" },
        h("button", {
          type: "button",
          className: `editor-tab${tab === "pipeline" ? " is-active" : ""}`,
          "aria-pressed": tab === "pipeline",
          onClick: () => setTab("pipeline"),
        }, "Pipeline"),
        h("button", {
          type: "button",
          className: `editor-tab${tab === "relations" ? " is-active" : ""}`,
          "aria-pressed": tab === "relations",
          onClick: () => setTab("relations"),
        }, "Relations"),
      ),
    ),
    h("section", { className: "editor-view", hidden: tab !== "pipeline" },
      missing
        ? h("p", { className: "status" }, name ? `No pipeline named \`${name}\` in this config.` : "Open a pipeline from the config view first.")
        : h(PipelineEditor, { state, name, onSaved: setState, onDirtyChange: setDirty })),
    h("section", { className: "editor-view", hidden: tab !== "relations" },
      h(RelationGraph, { relation: state.config?.relation })),
  );
}
