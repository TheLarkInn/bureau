import React, { useEffect, useId, useRef, useState } from "react";
import { getNodesBounds, Panel, useReactFlow, useStore, useViewport } from "@xyflow/react";
import { GRAPH_STATE_LABELS, initialGraphViewport, nextAttention, searchGraphItems } from "./graph-presentation.mjs";

const h = React.createElement;

export function GraphStateBadge({ state = "design" }) {
  const known = Object.hasOwn(GRAPH_STATE_LABELS, state) ? state : "unknown";
  return h("span", { className: `graph-state graph-state--${known}` },
    h("span", { className: "graph-state-dot", "aria-hidden": true }),
    GRAPH_STATE_LABELS[known]);
}

export function GraphTools({ items, selectedId, onSelect, label = "steps" }) {
  const flow = useReactFlow();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const input = useRef(null);
  const [trigger, setTrigger] = useState(null);
  useInitialGraphView(flow, trigger);
  const contentId = useId();
  const results = searchGraphItems(items, query);
  const attention = items.filter((item) => item.attention);
  const select = (item) => {
    onSelect?.(item.id);
    const node = flow.getNode(item.id);
    if (node) {
      flow.setCenter(node.position.x + (node.measured?.width ?? 240) / 2,
        node.position.y + (node.measured?.height ?? 112) / 2, { zoom: 1 });
    }
  };
  useEffect(() => {
    const find = (event) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "k"
          && trigger?.getClientRects().length) {
        event.preventDefault();
        setOpen(true);
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", find);
    return () => window.removeEventListener("keydown", find);
  }, [trigger]);
  useEffect(() => {
    if (open) {
      input.current?.focus();
    }
  }, [open]);
  const close = () => {
    setOpen(false);
    trigger?.focus();
  };
  return h(React.Fragment, null,
    h(Panel, { position: "top-left", className: "graph-navigator", onKeyDown: (event) => {
      if (event.key === "Escape" && open) {
        event.stopPropagation();
        close();
      }
    } },
    h("button", { type: "button", className: "graph-navigator-toggle", ref: setTrigger,
      title: "Find by name, type, or status (Ctrl/Cmd+K)", "aria-keyshortcuts": "Control+k Meta+k",
      "aria-expanded": open, "aria-controls": contentId, onClick: () => setOpen(!open) },
    `Find ${label}`, h("span", { className: "graph-tools-summary" }, String(items.length))),
    open ? h("div", { id: contentId, className: "graph-navigator-content" },
      h("input", { ref: input, className: "graph-search", type: "search", value: query,
        "aria-label": `Search ${label}`, placeholder: "Name, type, or status",
        onChange: (event) => setQuery(event.target.value),
        onKeyDown: (event) => {
          if (event.key === "Enter" && results[0]) {
            event.preventDefault();
            select(results[0]);
          }
        } }),
      h("p", { className: "graph-tools-summary", role: "status" }, `${results.length} ${label}`),
      results.length ? h("ul", { className: "graph-search-results" }, results.map((item) =>
        h("li", { key: item.id }, h("button", { type: "button", className: "graph-search-result",
          "aria-current": item.id === selectedId ? "true" : undefined, onClick: () => select(item) },
        h("strong", {}, item.name), h("span", {}, item.kind), item.state ? h(GraphStateBadge, { state: item.state }) : null))))
        : h("p", { className: "graph-search-empty" }, items.length ? "No matching steps or relations." : `No ${label} in this graph.`),
    ) : null,
    attention.length ? h("div", { className: "graph-attention" },
      h("span", { role: "status" }, `${attention.length} need attention`),
      h("button", { type: "button", className: "graph-attention-review",
        onClick: () => select(nextAttention(items, selectedId)) }, "Review next")) : null),
    h(GraphCamera));
}

function useInitialGraphView(flow, trigger) {
  // Read-only controlled graphs do not copy measurement changes into user nodes.
  const initialized = useStore((state) => state.nodeLookup.size > 0
    && [...state.nodeLookup.values()].every((node) => node.hidden
      || (node.measured?.width > 0 && node.measured?.height > 0)));
  const framed = useRef(false);
  useEffect(() => {
    const surface = trigger?.closest(".react-flow");
    if (!initialized || !flow.viewportInitialized || !surface || framed.current) {
      return undefined;
    }
    let frame;
    const arrange = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const box = surface.getBoundingClientRect();
        if (!framed.current && box.width > 0 && box.height > 0) {
          const nodes = flow.getNodes().filter((node) => !node.hidden).map((node) => flow.getInternalNode(node.id));
          flow.setViewport(initialGraphViewport(getNodesBounds(nodes), box.width, box.height))
            .then((applied) => { framed.current = applied; });
        }
      });
    };
    const observer = new ResizeObserver(arrange);
    observer.observe(surface);
    arrange();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [flow, initialized, trigger]);
}

function GraphCamera() {
  const flow = useReactFlow();
  const { zoom } = useViewport();
  return h(Panel, { position: "bottom-right", className: "graph-camera" },
    h("div", { role: "group", "aria-label": "Graph view controls" },
      h("button", { type: "button", "aria-label": "Zoom out", disabled: zoom <= 0.2, onClick: () => flow.zoomOut() }, "-"),
      h("button", { type: "button", className: "graph-zoom-value", "aria-label": "Actual size",
        title: "Actual size", onClick: () => flow.zoomTo(1) }, `${Math.round(zoom * 100)}%`),
      h("button", { type: "button", "aria-label": "Zoom in", disabled: zoom >= 3, onClick: () => flow.zoomIn() }, "+"),
      h("button", { type: "button", "aria-label": "Fit graph", onClick: () => flow.fitView({ padding: 0.22, maxZoom: 1 }) }, "Fit")),
    h("p", { className: "graph-help" }, "Drag to pan. Scroll to zoom."));
}
