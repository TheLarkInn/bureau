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

function EditorApp() {
  const [state, setState] = useState(null);
  const [tab, setTab] = useState("pipeline");

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
      h("div", {}, h("h1", {}, "Pipeline editor"), h("p", { className: "summary" }, name ?? state.dir)),
      h(
        "nav",
        { className: "editor-tabs" },
        h("button", { type: "button", className: tab === "pipeline" ? "is-active" : "", onClick: () => setTab("pipeline") }, "Pipeline"),
        h("button", { type: "button", className: tab === "relations" ? "is-active" : "", onClick: () => setTab("relations") }, "Relations"),
      ),
    ),
    tab === "relations"
      ? h(RelationGraph, { relation: state.config?.relation })
      : missing
        ? h("p", { className: "status" }, name ? `No pipeline named \`${name}\` in this config.` : "Open a pipeline from the config view first.")
        : h(PipelineEditor, { state, name, onSaved: setState }),
  );
}
