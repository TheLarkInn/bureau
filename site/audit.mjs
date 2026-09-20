import { axeSource, browserTools } from "./browser-tools.mjs";
import { onlyLocalRequests, temporaryPreview } from "./preview.mjs";

export const auditSchema = "bureau-site-check-v1";
export const viewports = [320, 375, 768, 1024, 1280, 1536];
export const auditKinds = ["accessibility", "responsive"];
const maximumFindings = 20;

export function reportStatus(report) {
  if (report.schema !== auditSchema || !auditKinds.includes(report.kind)
    || report.complete !== true || !Number.isSafeInteger(report.checks) || report.checks < 1
    || !Array.isArray(report.findings) || report.error) return 2;
  return report.findings.length ? 1 : 0;
}

export function incompleteReport(kind, message, checks = 0) {
  return {
    schema: auditSchema, kind, complete: false, checks, findings: [],
    error: { code: "incomplete", message: String(message).slice(0, 2000) },
  };
}

export function addFinding(report, id, title, detail, path = "site/src/index.html") {
  if (!/^[A-Za-z0-9._:-]{1,160}$/u.test(id) || !/^site\/src\/[A-Za-z0-9/_.-]+$/u.test(path)
    || path.split("/").some((part) => part === "." || part === "..")) {
    throw new Error("Invalid audit finding identity or source path.");
  }
  const duplicate = report.findings.some((item) => item.path === path
    && item.title === title.slice(0, 160) && item.detail === detail.slice(0, 4000));
  if (duplicate) return;
  if (report.findings.length >= maximumFindings) throw new Error("Finding limit reached; audit is incomplete.");
  report.findings.push({ id, title: title.slice(0, 160), detail: detail.slice(0, 4000), path });
}

async function accessibility(page, report, prefix, axe) {
  await page.evaluate(axe);
  const result = await page.evaluate(async () => {
    const timeout = new Promise((_, reject) => {
      setTimeout(() => reject(new Error("Accessibility scan exceeded 15 seconds.")), 15_000);
    });
    return Promise.race([window.axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] },
      resultTypes: ["violations", "passes", "incomplete"],
    }), timeout]);
  });
  report.checks += result.passes.length + result.violations.length;
  for (const violation of result.violations) {
    const detail = violation.nodes.slice(0, 8).map((node) =>
      `${node.target.join(" ")}: ${node.failureSummary}`).join("\n");
    addFinding(report, `a11y-${prefix}-${violation.id}`, violation.help, detail);
  }
  if (result.incomplete.length) {
    for (const rule of result.incomplete) {
      addFinding(report, `a11y-${prefix}-manual-${rule.id}`, `Manual review required: ${rule.help}`,
        rule.nodes.slice(0, 6).map((node) => `${node.target.join(" ")}: ${JSON.stringify({
          failure: node.failureSummary, checks: [...node.any, ...node.all, ...node.none],
        })}`).join("\n"));
    }
    throw new Error(`axe requires manual review in ${prefix}: ${result.incomplete.map(({ id }) => id).join(", ")}`);
  }
}

async function auditAccessibility(page, report, key, axe, url) {
  for (const state of ["pass", "repair", "budget"]) {
    await page.locator(`[data-outcome="${state}"]`).click();
    await accessibility(page, report, `${key}-desktop-${state}`, axe);
  }
  await page.setViewportSize({ width: 375, height: 812 });
  await page.locator('[data-outcome="pass"]').click();
  await accessibility(page, report, `${key}-mobile`, axe);
  await page.goto(`${url}not-a-page`, { waitUntil: "networkidle" });
  await accessibility(page, report, `${key}-404`, axe);
}

async function responsive(page, report, prefix) {
  const measurements = await page.evaluate(() => {
    const visible = (element) => element.getClientRects().length > 0;
    const label = (element) => `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ""}${[...element.classList].map((name) => `.${name}`).join("")}`;
    const targets = [...document.querySelectorAll("a, button")].filter(visible)
      .filter((element) => getComputedStyle(element).display !== "inline");
    return {
      viewport: window.innerWidth,
      width: document.documentElement.scrollWidth,
      clipped: [...document.querySelectorAll("main section, main figure, pre, .scenario")]
        .filter(visible).filter((element) => {
          const bounds = element.getBoundingClientRect();
          return bounds.left < -1 || bounds.right > window.innerWidth + 1 || element.scrollWidth > element.clientWidth + 1;
        }).map(label),
      smallTargets: targets.filter((element) => {
        const bounds = element.getBoundingClientRect();
        return bounds.width < 24 || bounds.height < 24;
      }).map(label),
    };
  });
  report.checks += 3;
  if (measurements.width > measurements.viewport + 1) {
    addFinding(report, `responsive-${prefix}-overflow`, "Page overflows the viewport",
      `${measurements.width}px content in a ${measurements.viewport}px viewport.`, "site/src/assets/layout.css");
  }
  if (measurements.clipped.length) {
    addFinding(report, `responsive-${prefix}-clipped`, "Content is horizontally clipped",
      measurements.clipped.join(", "), "site/src/assets/layout.css");
  }
  if (measurements.smallTargets.length) {
    addFinding(report, `responsive-${prefix}-targets`, "Standalone interactive targets are smaller than 24px",
      measurements.smallTargets.join(", "), "site/src/assets/base.css");
  }
}

async function auditResponsive(page, report, key) {
  for (const width of viewports) {
    await page.setViewportSize({ width, height: width < 700 ? 812 : 960 });
    for (const state of ["pass", "repair", "budget"]) {
      await page.locator('[data-filter="all"]').click();
      await page.locator(`[data-outcome="${state}"]`).click();
      await responsive(page, report, `${key}-${width}-${state}`);
    }
    await page.locator('[data-filter="operate"]').click();
    await page.locator('[data-arch="aarch64"]').click();
    await responsive(page, report, `${key}-${width}-filtered-arm64`);
  }
}

async function inspectBase(browser, base, report, axe) {
  const preview = await temporaryPreview(base);
  let context;
  try {
    context = await browser.newContext({
      viewport: { width: 1440, height: 960 }, reducedMotion: "reduce",
      locale: "en-US", timezoneId: "UTC", colorScheme: "light", serviceWorkers: "block",
    });
    const failures = [];
    await onlyLocalRequests(context, new URL(preview.url).origin, failures);
    const page = await context.newPage();
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(15_000);
    page.on("pageerror", (error) => failures.push(error.message));
    page.on("response", (response) => {
      if (response.status() >= 400 && response.url() !== `${preview.url}not-a-page`) {
        failures.push(`Resource returned ${response.status()}: ${new URL(response.url()).pathname}`);
      }
    });
    page.on("console", (message) => {
      const expected404 = page.url() === `${preview.url}not-a-page`
        && message.text() === "Failed to load resource: the server responded with a status of 404 (Not Found)";
      if (message.type() === "error" && !expected404) failures.push(message.text());
    });
    await page.goto(preview.url, { waitUntil: "networkidle" });
    const key = base === "/" ? "root" : "repo";
    if (report.kind === "accessibility") await auditAccessibility(page, report, key, axe, preview.url);
    else await auditResponsive(page, report, key);
    if (failures.length) throw new Error(`Page errors: ${failures.slice(0, 5).join("; ")}`);
  } finally {
    try { if (context) await context.close(); }
    finally { await preview.close(); }
  }
}

export async function audit(kind) {
  if (!auditKinds.includes(kind)) throw new Error("Audit kind must be accessibility or responsive.");
  const report = { schema: auditSchema, kind, complete: false, checks: 0, findings: [], bases: ["/bureau/", "/"] };
  let browser;
  let deadline;
  try {
    const axe = kind === "accessibility" ? axeSource() : undefined;
    browser = await browserTools().chromium.launch({ timeout: 15_000 });
    deadline = setTimeout(() => { void browser.close(); }, 110_000);
    for (const base of report.bases) await inspectBase(browser, base, report, axe);
    if (report.checks < 1) throw new Error("No checks completed.");
    report.complete = true;
    report.findings.sort((left, right) => left.id.localeCompare(right.id, "en"));
  } catch (error) {
    report.error = { code: "incomplete", message: error.message.slice(0, 2000) };
  } finally {
    clearTimeout(deadline);
    if (browser) await browser.close();
  }
  return report;
}
