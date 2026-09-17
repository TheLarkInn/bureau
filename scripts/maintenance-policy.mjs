import { readFile } from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";

import { AUTHOR, CATEGORIES, requireValue } from "./maintenance-contract.mjs";
import { GiB } from "./maintenance-resources.mjs";

export function validatePolicy(policy, requireIdentity = true) {
  requireValue(policy?.schema === "bureau-maintenance-policy-v1", "invalid maintenance policy");
  requireValue(Array.isArray(policy.backing_paths)
    && policy.backing_paths.every((path) => typeof path === "string" && isAbsolute(path)),
  "backing_paths must contain explicit absolute filesystem paths");
  requireValue(typeof policy.cargo_target === "string" && isAbsolute(policy.cargo_target),
    "cargo_target must be an absolute disposable cache path");
  for (const key of ["site_tools", "browser_path"]) {
    requireValue(typeof policy[key] === "string" && isAbsolute(policy[key]),
      `${key} must be an absolute, pre-provisioned read-only path`);
  }
  requireValue(Number.isSafeInteger(policy.cargo_cache_max_bytes)
    && policy.cargo_cache_max_bytes > 0 && policy.cargo_cache_max_bytes <= 8 * GiB,
  "Cargo cache must have a positive ceiling no larger than 8 GiB");
  if (!requireIdentity) return policy;
  requireValue(policy.issuer_login === AUTHOR
    && Number.isSafeInteger(policy.issuer_id) && policy.issuer_id > 0,
  "review and pin the maintainer's numeric GitHub identity before enabling maintenance");
  const numbers = CATEGORIES.map((category) => policy.source_issues?.[category]);
  requireValue(numbers.every((number) => Number.isSafeInteger(number) && number > 0)
    && new Set(numbers).size === CATEGORIES.length,
  "review and pin three distinct source issue numbers before enabling maintenance");
  return policy;
}

export async function loadPolicy(root = process.cwd(), requireIdentity = true) {
  const bytes = await readFile(resolve(root, "deployment", "maintenance-policy.json"), "utf8");
  requireValue(bytes.length <= 8192, "maintenance policy exceeds the byte limit");
  return validatePolicy(JSON.parse(bytes), requireIdentity);
}

export function issuer(object, policy) {
  requireValue(object?.user?.login === policy.issuer_login && object.user.id === policy.issuer_id,
    "forge identity differs from the reviewed numeric issuer");
}
