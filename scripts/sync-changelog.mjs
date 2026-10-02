// Render website/docs/changelog.md from the root CHANGELOG.md so the two copies
// cannot drift. The root CHANGELOG.md is the single source of truth; the website
// page is generated and must not be edited by hand.
//
// Run manually with `node scripts/sync-changelog.mjs` (or `npm run sync-changelog`
// in website/). The website build runs it automatically through the `prebuild`
// hook in website/package.json, so a published site always matches CHANGELOG.md.
//
// The only transform is on links: a repo-relative `docs/<file>` markdown link is
// rewritten to an absolute GitHub blob URL. That keeps the link valid on GitHub
// (where CHANGELOG.md is read directly) and on the VitePress site (where a
// repo-relative path would otherwise be a dead internal link), and matches the
// GitHub-blob convention the site already uses for docs that are not published.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const src = resolve(root, "CHANGELOG.md");
const dest = resolve(root, "website", "docs", "changelog.md");

const REPO_BLOB = "https://github.com/pwilkin/ROCprofGUI/blob/main";

const header = `<!-- GENERATED FILE - do not edit by hand.
     Rendered from CHANGELOG.md by scripts/sync-changelog.mjs.
     Edit CHANGELOG.md and re-run the generator (the website build does this
     automatically via the prebuild hook). -->

`;

const source = readFileSync(src, "utf8");
const body = source.replace(
  /\]\((?:\.\/|\/)?(docs\/[^)]+)\)/g,
  `](${REPO_BLOB}/$1)`,
);

writeFileSync(dest, header + body.replace(/\n*$/, "\n"));
console.log(`sync-changelog: wrote ${dest} from CHANGELOG.md`);
