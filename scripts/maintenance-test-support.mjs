import {
  AUTHOR, LABELS, categoryLabel, evidence, intentBlock, issueUrl, labels, sourceMarker,
} from "./maintenance-contract.mjs";

export const COMMIT = "1".repeat(40);
export const CYCLE = "2026-09-17T12:00:00Z";
export const POLICY = {
  schema: "bureau-maintenance-policy-v1", issuer_login: AUTHOR, issuer_id: 17,
  source_issues: { chaos: 7, "site-accessibility": 8, "site-responsive": 9 },
  backing_paths: [], cargo_target: "/cache/cargo", cargo_cache_max_bytes: 8 * 1024 ** 3,
  site_tools: "/opt/site-tools", browser_path: "/opt/browsers",
};

export function fixture(category = "chaos", findings = undefined) {
  const number = POLICY.source_issues[category];
  const intent = { schema: "bureau-maintenance-intent-v1", category, commit: COMMIT, cycle: CYCLE };
  const source = {
    number, html_url: issueUrl(number), state: "open",
    user: { login: AUTHOR, id: 17 },
    labels: [categoryLabel(category), LABELS.approved, LABELS.ready, LABELS.scan, "human-context"],
    body: `${sourceMarker(category)}\n\nHuman instructions stay here.\n\n${intentBlock(intent)}\n`,
  };
  const pin = { id: `TheLarkInn/bureau#${number}`, category, commit: COMMIT, cycle: CYCLE };
  const defaultFindings = [{ id: "invariant", title: "Invariant failure", detail: "Reproduce this failure.",
    path: category === "chaos" ? "crates/bureau/tests/maintenance_chaos.rs" : "site/src/index.html" }];
  const value = evidence(pin, 8, findings ?? defaultFindings);
  const request = { schema: "v2", item: { external_id: pin.id, url: source.html_url, body: source.body },
    inputs: { maintenance_source: pin, maintenance_evidence: value } };
  return { source, value, request, intent };
}

export function fakeForge(source) {
  const issues = new Map([[source.number, structuredClone(source)]]);
  const comments = new Map([[source.number, []]]);
  const writes = [];
  let failure = null;
  let readHook = null;
  const copy = (value) => structuredClone(value);
  const api = {
    issues: async () => copy([...issues.values()]),
    comments: async (number) => copy(comments.get(number) ?? []),
    async issue(number) {
      if (readHook) readHook(number);
      if (!issues.has(number)) throw new Error("missing fake issue");
      return copy(issues.get(number));
    },
    async request(method, path, body) {
      if (path === "/user" && method === "GET") return { login: AUTHOR, id: 17 };
      writes.push({ method, path, body: copy(body) });
      let result;
      if (path === "/issues" && method === "POST") {
        const number = 42;
        result = { ...body, number, html_url: issueUrl(number), user: { login: AUTHOR, id: 17 },
          state: "open", labels: [] };
        issues.set(number, result);
      } else {
        const match = /^\/issues\/(\d+)(.*)$/u.exec(path);
        if (!match) throw new Error(`unexpected fake route: ${method} ${path}`);
        const number = Number(match[1]);
        const issue = issues.get(number);
        if (method === "PATCH" && !match[2]) Object.assign(issue, body);
        else if (method === "POST" && match[2] === "/comments") {
          result = { ...body, user: { login: AUTHOR, id: 17 } };
          comments.get(number).push(result);
        } else if (method === "POST" && match[2] === "/labels") {
          issue.labels = [...new Set([...labels(issue), ...body.labels])];
        } else if (method === "DELETE" && match[2].startsWith("/labels/")) {
          const label = decodeURIComponent(match[2].slice("/labels/".length));
          issue.labels = labels(issue).filter((value) => value !== label);
        } else throw new Error(`unexpected fake mutation: ${method} ${path}`);
        result ??= issue;
      }
      if (failure?.method === method && failure.path === path) {
        failure = null;
        throw new Error("lost fake response after effect");
      }
      return copy(result);
    },
  };
  return { api, issues, comments, writes,
    loseResponse(method, path) { failure = { method, path }; },
    onRead(hook) { readHook = hook; } };
}

export function libtestRun(failed = false) {
  return { code: failed ? 101 : 0, signal: null, problem: null, stderr: "",
    stdout: `running 1 test\nBUREAU_CHAOS_SEED=0\n`
      + `test result: ${failed ? "FAILED" : "ok"}. ${failed ? 0 : 1} passed; ${failed ? 1 : 0} failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n` };
}
