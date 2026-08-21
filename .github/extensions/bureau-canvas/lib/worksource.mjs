// Deriving a work source from a forge URL.
//
// A developer already has the board or issues page open; pasting its URL is
// the shortest path to `work: { forge, source, filter }`. This module is the
// whole of that derivation: pure, offline, no DOM and no network, so every
// case below is testable in milliseconds.
//
// Two rules keep it honest. The filter it emits is forge-native and is
// passed to the forge verbatim — this never invents a filter language, and
// bureau never parses one (DESIGN.md section 6). And it reports what it had
// to infer: `exact` is true only when the filter came from the URL itself,
// and `notes` name every assumption, because a filter that silently means
// something other than the page you copied is worse than no filter at all.

/** WIQL macros, which are written bare rather than quoted. */
const MACROS = new Map([
  ["@me", "@Me"],
  ["@today", "@Today"],
  ["@currentiteration", "@CurrentIteration"],
  ["@project", "@Project"],
]);

/** Fields whose idiomatic WIQL operator is not equality. */
const OPERATORS = new Map([
  ["System.AreaPath", "UNDER"],
  ["System.IterationPath", "UNDER"],
  ["System.Tags", "CONTAINS"],
]);

const GITHUB_DEFAULT = "is:open";
const ADO_DEFAULT = "[System.State] = 'Active'";

/**
 * Derives `{ forge, source, filter }` from a pasted forge URL.
 *
 * Returns `{ ok: false, reason }` when the URL is not one this understands;
 * the caller is expected to offer the manual fields instead of guessing.
 */
export function deriveWorkSource(input) {
  const text = String(input ?? "").trim();
  if (!text) {
    return { ok: false, reason: "paste a URL from your board or issues page" };
  }
  let url;
  try {
    url = new URL(text);
  } catch {
    return { ok: false, reason: `not a URL: ${text}` };
  }
  const host = url.hostname.toLowerCase();
  if (host === "github.com" || host === "www.github.com") {
    return fromGithub(url);
  }
  if (host === "dev.azure.com" || host.endsWith(".visualstudio.com")) {
    return fromAzureDevOps(url, host);
  }
  return { ok: false, reason: `unrecognized host \`${url.hostname}\` — expected github.com, dev.azure.com, or an *.visualstudio.com organization` };
}

// ---------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------

/**
 * GitHub scopes issues to a repository, so the source is `owner/name` and the
 * filter is the page's own search syntax.
 */
function fromGithub(url) {
  const parts = segments(url);
  if (parts[0] === "orgs" || parts[0] === "users") {
    return { ok: false, reason: "that is an organization-wide page; bureau queries one repository, so open the repository's issues instead" };
  }
  if (parts.length < 2) {
    return { ok: false, reason: "no owner/repository in that GitHub URL" };
  }
  const source = `${parts[0]}/${parts[1]}`;
  const query = url.searchParams.get("q");
  if (query) {
    return found("github", source, query, true, []);
  }
  const label = labelFrom(parts);
  if (label) {
    return found("github", source, `${GITHUB_DEFAULT} label:${quoteLabel(label)}`, false, [
      "built from the label in the URL; the page itself carried no search query",
    ]);
  }
  return found("github", source, GITHUB_DEFAULT, false, [
    "the URL carried no search query, so this matches every open item — narrow it before saving",
  ]);
}

/** `/owner/repo/labels/<name>` names a label without a search query. */
function labelFrom(parts) {
  const at = parts.indexOf("labels");
  return at >= 0 ? parts[at + 1] ?? null : null;
}

function quoteLabel(label) {
  return label.includes(" ") ? `"${label}"` : label;
}

// ---------------------------------------------------------------------------
// Azure DevOps
// ---------------------------------------------------------------------------

/**
 * Azure DevOps hangs work items off a project rather than a repository, which
 * is exactly why `source` is an opaque forge-native string and not a repo
 * name: the work and the code are independent (DESIGN.md section 6).
 */
function fromAzureDevOps(url, host) {
  const parts = segments(url);
  // dev.azure.com carries the organization in the path; the older
  // <org>.visualstudio.com carries it in the hostname.
  const offset = host === "dev.azure.com" ? 1 : 0;
  const project = parts[offset];
  if (!project) {
    return { ok: false, reason: "no project in that Azure DevOps URL" };
  }
  const team = teamFrom(parts, offset);
  const source = team ? `${project}/${team}` : project;
  const { filter, notes } = adoFilter(url, parts);
  return found("ado", source, filter, notes.length === 0, notes);
}

/** `.../_boards/board/t/<team>/...` and `.../<team>/_boards/...` both occur. */
function teamFrom(parts, offset) {
  const marker = parts.indexOf("t");
  if (marker >= 0 && parts[marker + 1] && parts[marker - 1] === "board") {
    return parts[marker + 1];
  }
  const boards = parts.findIndex((part) => part.startsWith("_"));
  return boards > offset + 1 ? parts[offset + 1] : null;
}

function adoFilter(url, parts) {
  const clauses = [];
  for (const [field, value] of url.searchParams) {
    if (!field.includes(".")) {
      continue; // view state (`_a`, `type`), not a work-item field
    }
    clauses.push(clauseFor(field, value.split(",").filter(Boolean)));
  }
  const notes = adoNotes(parts, clauses.length);
  return { filter: clauses.length ? clauses.join("\n  AND ") : ADO_DEFAULT, notes };
}

function adoNotes(parts, clauseCount) {
  const notes = [];
  if (parts.includes("_queries")) {
    notes.push("a saved query's clauses live in Azure DevOps, not in its URL — paste the query's WIQL, or open the board with its filters applied");
  } else if (parts.includes("_boards")) {
    notes.push("board column and swimlane rules are not in the URL, so this filter matches the board's backlog rather than one column");
  }
  if (clauseCount === 0) {
    notes.push("the URL carried no field filters, so this defaults to active items — narrow it before saving");
  }
  return notes;
}

/** One WIQL clause, using the operator that field actually takes. */
function clauseFor(field, values) {
  const operator = OPERATORS.get(field) ?? "=";
  if (values.length === 0) {
    return `[${field}] ${operator} ''`;
  }
  if (operator !== "=") {
    return orJoin(field, operator, values);
  }
  if (values.length === 1) {
    return `[${field}] = ${wiqlValue(values[0])}`;
  }
  // `IN` reads better but is not dependable with macros, so a set that mixes
  // them falls back to equality alternatives, which is always valid WIQL.
  return values.some(isMacro)
    ? orJoin(field, "=", values)
    : `[${field}] IN (${values.map(wiqlValue).join(", ")})`;
}

function orJoin(field, operator, values) {
  const parts = values.map((value) => `[${field}] ${operator} ${wiqlValue(value)}`);
  return parts.length === 1 ? parts[0] : `(${parts.join(" OR ")})`;
}

function isMacro(value) {
  return MACROS.has(decodeURIComponent(value).trim().toLowerCase());
}

function wiqlValue(value) {
  const decoded = decodeURIComponent(value).trim();
  const macro = MACROS.get(decoded.toLowerCase());
  return macro ?? `'${decoded.replaceAll("'", "''")}'`;
}

// ---------------------------------------------------------------------------
// shared
// ---------------------------------------------------------------------------

function segments(url) {
  return url.pathname.split("/").filter(Boolean).map(decode);
}

function decode(part) {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

function found(forge, source, filter, exact, notes) {
  return { ok: true, forge, source, filter, exact, notes };
}
