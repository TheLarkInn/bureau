// Import-free contract shared by the host action and browser editor.

export const FACTORY_PROFILE = "copilot-sdk-factory-v1";
const DIGEST = "^tree-sha256:[a-f0-9]{64}$";
const PATH = { type: "string", minLength: 1 };
const LIMITS = {
  max_concurrent_subagents: { type: "integer", minimum: 1, maximum: 500 },
  max_total_subagents: { type: "integer", minimum: 1, maximum: 4294967295 },
  timeout_seconds: { type: "number", exclusiveMinimum: 0 },
  max_ai_credits: { type: "number", exclusiveMinimum: 0 },
};

export const factorySchema = {
  type: "object",
  additionalProperties: false,
  required: ["name", "extension", "extension_digest", "model_credential", "runtime"],
  properties: {
    name: { type: "string", pattern: "^[A-Za-z0-9_-]+$" },
    extension: { type: "string", pattern: "^project:[A-Za-z0-9_-]+(?:\\.[A-Za-z0-9_-]+)*$" },
    extension_digest: { type: "string", pattern: DIGEST },
    model_credential: { type: "string", pattern: "^[A-Za-z0-9_-]+$",
      description: "Declared local credential reference for model authentication; never a token value." },
    metadata: PATH,
    runtime: {
      type: "object",
      additionalProperties: false,
      required: ["profile", "directory", "digest", "version", "executable", "dist"],
      properties: {
        profile: { type: "string", const: FACTORY_PROFILE,
          description: "Bureau's SDK capability contract, not an upstream release or version." },
        directory: PATH,
        digest: { type: "string", pattern: DIGEST },
        version: { type: "string", minLength: 1 },
        executable: PATH,
        cli: { type: ["string", "null"] },
        dist: PATH,
      },
    },
    args: { type: ["object", "null"], description: "Static factory arguments, not inputs_from or encoded JSON." },
    limits: { type: "object", additionalProperties: false, properties: LIMITS },
  },
};

function object(value) {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function structural(value, schema, path, found) {
  if (!object(value)) {
    found.push(`${path} must be an object`);
    return false;
  }
  for (const name of Object.keys(value)) {
    if (!Object.hasOwn(schema.properties, name)) {
      found.push(`${path}.${name} is not a supported field`);
    }
  }
  for (const name of schema.required ?? []) {
    if (!Object.hasOwn(value, name)) {
      found.push(`${path}.${name} is required`);
    }
  }
  return true;
}

function matches(value, expression) {
  return typeof value === "string" && new RegExp(expression, "u").test(value);
}

function safePath(value, absolute = false, current = false) {
  if (typeof value !== "string" || !value.trim() || value.includes("\0")) {
    return false;
  }
  const parts = value.split(/[\\/]/u);
  if (parts.includes("..")) {
    return false;
  }
  if (absolute) {
    return value.startsWith("/");
  }
  return !/^(?:[\\/]|[A-Za-z]:)/u.test(value)
    && (current || parts.some((part) => part && part !== "."));
}

function pathProblem(found, value, name, absolute = false, current = false) {
  if (!safePath(value, absolute, current)) {
    found.push(`${name} must be a safe ${absolute ? "absolute Linux" : "relative"} path without parent components`);
  }
}

function runtimeProblems(runtime, found) {
  if (!structural(runtime, factorySchema.properties.runtime, "runtime", found)) {
    return;
  }
  if (runtime.profile !== FACTORY_PROFILE) {
    found.push(`runtime.profile must be Bureau's SDK capability contract ${FACTORY_PROFILE}`);
  }
  if (!matches(runtime.digest, DIGEST)) {
    found.push("runtime.digest must be tree-sha256: followed by 64 lowercase hexadecimal digits");
  }
  if (typeof runtime.version !== "string" || !runtime.version.trim()) {
    found.push("runtime.version must be the exact qualified connect.version");
  }
  pathProblem(found, runtime.directory, "runtime.directory", true);
  pathProblem(found, runtime.executable, "runtime.executable");
  pathProblem(found, runtime.dist, "runtime.dist", false, true);
  if (runtime.cli != null) {
    pathProblem(found, runtime.cli, "runtime.cli");
  }
}

function limitProblems(limits, found) {
  if (!structural(limits, factorySchema.properties.limits, "limits", found)) {
    return;
  }
  for (const [name, value] of Object.entries(limits)) {
    const schema = LIMITS[name];
    if (!schema) {
      continue;
    }
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0
      || (schema.type === "integer" && (!Number.isInteger(value) || value > schema.maximum))) {
      const maximum = schema.maximum == null ? "" : ` no greater than ${schema.maximum}`;
      found.push(`limits.${name} must be a positive ${schema.type}${maximum}; null does not remove a ceiling`);
    }
  }
}

/** Structural hints only; Bureau validates the pinned full argsSchema before execution. */
export function factoryProblems(value) {
  const found = [];
  if (!structural(value, factorySchema, "copilot_factory", found)) {
    return found;
  }
  if (!matches(value.name, factorySchema.properties.name.pattern)) {
    found.push("factory name must use ASCII alphanumerics, hyphens, or underscores");
  }
  if (!matches(value.extension, factorySchema.properties.extension.pattern)) {
    found.push("factory extension must be project:<single safe directory name>");
  }
  if (!matches(value.extension_digest, DIGEST)) {
    found.push("extension_digest must be tree-sha256: followed by 64 lowercase hexadecimal digits");
  }
  if (!matches(value.model_credential, factorySchema.properties.model_credential.pattern)) {
    found.push("model_credential must be a declared reference using ASCII alphanumerics, hyphens, or underscores");
  }
  if (Object.hasOwn(value, "metadata")) {
    pathProblem(found, value.metadata, "metadata");
  }
  if (Object.hasOwn(value, "args") && value.args !== null && !object(value.args)) {
    found.push("factory args must be a raw JSON object or null, separate from data inputs");
  }
  runtimeProblems(value.runtime, found);
  if (Object.hasOwn(value, "limits")) {
    limitProblems(value.limits, found);
  }
  return found;
}

export function stepFactoryProblems(step, steps = [], roles) {
  const value = step.fields?.copilotFactory;
  if (value == null) {
    return [];
  }
  const found = factoryProblems(value);
  if (step.kind !== "agent") {
    found.push("copilot_factory requires an agent step");
  }
  if (roles) {
    const role = roles.find((role) => role.name === step.fields.role);
    if (role?.adapter !== "copilot") {
      found.push("copilot_factory requires a Copilot role");
    } else if (!role.permissions?.includes("model:invoke")) {
      found.push("copilot_factory requires the role's model:invoke permission");
    }
  }
  if (steps.some((group) => group.kind === "concurrent" && group.fields?.members?.includes(step.name))) {
    found.push("local factories cannot be concurrent group members");
  }
  return found.map((message) => ({ step: step.name, message }));
}

export function newFactory() {
  return {
    name: "", extension: "project:", extension_digest: "", model_credential: "", metadata: "factory.json",
    runtime: { profile: FACTORY_PROFILE, directory: "", digest: "", version: "", executable: "", dist: "." },
    args: null,
  };
}
