import React, { useEffect, useRef, useState } from "react";

import { FACTORY_PROFILE, factoryProblems, factorySchema, newFactory } from "../copilot-factory.mjs";

const h = React.createElement;
const PROVIDER_FIELDS = [
  ["name", "Factory name", "review"],
  ["extension", "Provider source", "project:review"],
  ["extension_digest", "Approved provider digest", "tree-sha256:..."],
  ["metadata", "Provider metadata file", "factory.json"],
  ["model_credential", "Model credential reference", "copilot-model"],
];
const RUNTIME_FIELDS = [
  ["directory", "Qualified runtime directory", "/opt/copilot-qualified"],
  ["digest", "Approved runtime digest", "tree-sha256:..."],
  ["version", "Exact runtime version", "connect.version"],
  ["executable", "Host executable", "bin/node"],
  ["cli", "CLI entrypoint (optional)", "dist/index.js"],
  ["dist", "CLI distribution directory", "dist"],
];
const LIMIT_FIELDS = [
  ["max_concurrent_subagents", "Concurrent subagents", "1"],
  ["max_total_subagents", "Total subagents", "1"],
  ["timeout_seconds", "Cumulative active seconds", "any"],
  ["max_ai_credits", "Cumulative AI credits (soft)", "any"],
];

function textField(name, label, value, onChange, placeholder) {
  return h("label", { key: name }, label, h("input", {
    className: "form-control form-control--mono",
    "aria-label": label, value: value ?? "", placeholder,
    onChange: (event) => onChange(event.target.value),
  }));
}

function setPart(value, section, name, replacement) {
  const next = structuredClone(value);
  const target = section ? (next[section] ??= {}) : next;
  if (replacement === undefined) {
    delete target[name];
  } else {
    target[name] = replacement;
  }
  if (section === "limits" && Object.keys(target).length === 0) {
    delete next.limits;
  }
  return next;
}

function jsonText(value) {
  return typeof value === "string" ? value : JSON.stringify(value ?? null, null, 2);
}

function JsonArguments({ value, onChange }) {
  const [text, setText] = useState(() => jsonText(value));
  const ownValue = useRef(value);
  useEffect(() => {
    if (ownValue.current !== value) {
      setText(jsonText(value));
      ownValue.current = value;
    }
  }, [value]);
  const edit = (text) => {
    let next = text;
    try {
      const parsed = JSON.parse(text);
      if (parsed === null || (typeof parsed === "object" && !Array.isArray(parsed))) {
        next = parsed;
      }
    } catch {
      // Keep invalid draft text visible and unsavable until it becomes object/null.
    }
    ownValue.current = next;
    setText(text);
    onChange(next);
  };
  return h("label", {}, "Static factory arguments (JSON object or null)", h("textarea", {
    className: "form-control form-control--mono editor-textarea",
    "aria-label": "Static factory arguments", value: text,
    onChange: (event) => edit(event.target.value),
  }));
}

function RuntimeFields({ value, onChange }) {
  return h("details", { className: "editor-factory-runtime" },
    h("summary", {}, "Qualified runtime and SDK"),
    h("p", { className: "muted" }, "An operator-qualified, preprovisioned bundle is required. Protocol 3 alone does not qualify a release."),
    h("label", {}, "Bureau SDK capability profile", h("input", {
      className: "form-control form-control--mono", "aria-label": "Factory SDK capability profile",
      readOnly: true, value: value.runtime?.profile ?? FACTORY_PROFILE,
    })),
    h("p", { className: "muted" }, "This names Bureau's required SDK capabilities, not an upstream release or version."),
    RUNTIME_FIELDS.map(([name, label, placeholder]) => textField(name, label, value.runtime?.[name],
      (text) => onChange(setPart(value, "runtime", name, name === "cli" && text === "" ? undefined : text)), placeholder)));
}

function LimitFields({ value, onChange }) {
  return h("details", {},
    h("summary", {}, "Native ceilings (optional)"),
    h("p", { className: "muted" }, "Omitted ceilings retain the factory/runtime policy. Resuming does not reset or increase them."),
    LIMIT_FIELDS.map(([name, label, step]) => h("label", { key: name }, label, h("input", {
      type: "number", min: factorySchema.properties.limits.properties[name].minimum ?? 0,
      max: factorySchema.properties.limits.properties[name].maximum, step, "aria-label": label,
      className: "form-control form-control--mono",
      value: value.limits?.[name] ?? "", placeholder: "Not overridden",
      onChange: (event) => onChange(setPart(value, "limits", name,
        event.target.value === "" ? undefined : Number(event.target.value))),
    }))));
}

export function FactorySettings({ value, eligible, onChange }) {
  if (!eligible && value == null) {
    return null;
  }
  const errors = value == null ? [] : factoryProblems(value);
  return h("fieldset", { className: "editor-edges editor-factory" },
    h("legend", {}, "Local Copilot factory"),
    h("label", { className: "editor-check" },
      h("input", {
        type: "checkbox", "aria-label": "Use a local Copilot factory",
        checked: value != null, disabled: !eligible && value == null,
        onChange: (event) => onChange(event.target.checked ? newFactory() : null),
      }), "Use a runtime factory instead of ACP"),
    value == null ? null : h(React.Fragment, {},
      h("p", { className: "muted" }, "Review provider code, static args, and ceilings as executable authority. Bureau uses committed configuration; saving this draft does not run a factory."),
      !eligible ? h("p", { role: "alert", className: "editor-hints" }, "Choose a Copilot role or remove the factory configuration.") : null,
      PROVIDER_FIELDS.map(([name, label, placeholder]) => textField(name, label, value[name],
        (text) => onChange(setPart(value, null, name, text)), placeholder)),
      h("p", { className: "muted" }, "Model credential reference names a declared source in local settings.yaml. Never paste a token. This does not grant forge tools or enable ambient login."),
      h(JsonArguments, { value: value.args, onChange: (args) => onChange(setPart(value, null, "args", args)) }),
      h("p", { className: "muted" }, "Data inputs below remain StepRequest inputs; they do not interpolate into static factory arguments."),
      h(RuntimeFields, { value, onChange }),
      h(LimitFields, { value, onChange }),
      errors.length ? h("ul", { className: "editor-issues", "aria-label": "Factory configuration issues" },
        errors.map((message) => h("li", { key: message }, message))) : null));
}
