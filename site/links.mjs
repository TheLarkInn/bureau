const sourcePrefix = "https://github.com/TheLarkInn/bureau";

function documentIds(html) {
  const ids = [...html.matchAll(/\bid="([^"]+)"/gu)].map((match) => match[1]);
  if (new Set(ids).size !== ids.length) throw new Error("Duplicate HTML id.");
  return new Set(ids);
}

export function validateLinks(files, base) {
  const documents = new Map([...files]
    .filter(([name]) => name.endsWith(".html"))
    .map(([name, bytes]) => [name, { html: bytes.toString(), ids: documentIds(bytes.toString()) }]));
  let checked = 0;
  for (const [name, document] of documents) {
    for (const [, attribute, raw] of document.html.matchAll(/\b(href|src)="([^"]+)"/gu)) {
      const href = raw.replaceAll("&amp;", "&");
      const url = new URL(href, `https://site.invalid${base}${name}`);
      if (url.origin !== "https://site.invalid") {
        if (attribute === "src") throw new Error(`External asset in ${name}: ${href}`);
        if (!href.startsWith(`${sourcePrefix}/`) && href !== sourcePrefix) {
          if (!document.html.includes(`<link rel="canonical" href="${raw}">`)) {
            throw new Error(`Unexpected external resource in ${name}: ${href}`);
          }
        }
      } else {
        if (!url.pathname.startsWith(base)) throw new Error(`Link escapes base in ${name}: ${href}`);
        const target = url.pathname.slice(base.length) || "index.html";
        if (!files.has(target)) throw new Error(`Missing local target in ${name}: ${href}`);
        if (url.hash && !documents.get(target)?.ids.has(decodeURIComponent(url.hash.slice(1)))) {
          throw new Error(`Missing anchor in ${name}: ${href}`);
        }
      }
      checked += 1;
    }
  }
  if (checked === 0) throw new Error("No page links were checked.");
  return checked;
}
