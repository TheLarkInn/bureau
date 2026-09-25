import { AUTHOR, REPOSITORY, requireValue } from "./maintenance-contract.mjs";

const BASE = `https://api.github.com/repos/${REPOSITORY}`;
const MAX_BODY = 2 * 1024 * 1024;

async function jsonBody(response) {
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    requireValue(size <= MAX_BODY, "forge response exceeds the byte limit");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

export function github({ token, fetchImpl = fetch } = {}) {
  const headers = { Accept: "application/vnd.github+json",
    "User-Agent": "bureau-maintenance", "X-GitHub-Api-Version": "2022-11-28" };
  if (token) headers.Authorization = `Bearer ${token}`;
  async function request(method, path, body) {
    const route = /^\/issues(?:\?(?:state=all&)?per_page=100&page=[1-5]|\/[1-9]\d*(?:\/comments(?:\?per_page=100&page=[1-5])?|\/labels(?:\/[a-zA-Z0-9%:_-]+)?)?)?$/u;
    requireValue(route.test(path) || path === "/user", "unsupported forge route");
    requireValue(["GET", "POST", "PATCH", "DELETE"].includes(method), "unsupported forge method");
    requireValue(method === "GET" || token, "forge mutation requires an explicit credential");
    const url = path === "/user" ? "https://api.github.com/user" : `${BASE}${path}`;
    const response = await fetchImpl(url, {
      method, headers: { ...headers, "Content-Type": "application/json",
        // Ask shared caches to revalidate; verification never depends on it.
        ...(method === "GET" && { "Cache-Control": "no-cache" }) },
      body: body === undefined ? undefined : JSON.stringify(body),
      redirect: "error", signal: AbortSignal.timeout(10_000),
    });
    requireValue(response.ok, `forge ${method} ${path} returned HTTP ${response.status}`);
    return response.status === 204 ? null : jsonBody(response);
  }
  async function list(path) {
    const values = [];
    for (let page = 1; page <= 5; page += 1) {
      const batch = await request("GET", `${path}${path.includes("?") ? "&" : "?"}per_page=100&page=${page}`);
      requireValue(Array.isArray(batch) && batch.length <= 100, "invalid forge page");
      values.push(...batch);
      if (batch.length < 100) return values;
    }
    throw new Error("forge pagination exceeded five pages; narrow or archive with human review");
  }
  return {
    request,
    issue: (number) => request("GET", `/issues/${number}`),
    issues: () => list("/issues?state=all"),
    comments: (number) => list(`/issues/${number}/comments`),
    async verifyActor() {
      requireValue((await request("GET", "/user")).login === AUTHOR,
        "credential is not the approved maintainer identity");
    },
  };
}
