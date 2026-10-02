import { defineConfig } from "vitepress";

// ---------------------------------------------------------------------------
// BASE PATH — the one setting a human must get right for GitHub Pages.
//
// The site is served from https://<owner>.github.io/<repo>/, so the base must
// be "/<repo>/" — and GitHub Pages project URLs are case-sensitive. It
// defaults to "/ROCprofGUI/" (the repository name, exact case). If the
// repository is renamed, or served from a user/organisation root or a custom
// domain, override it with the DOCS_BASE environment variable at build time:
//
//   DOCS_BASE=/my-repo/ npm run build      # project pages under a subpath
//   DOCS_BASE=/        npm run build        # user/org root or custom domain
//
// The Pages workflow (.github/workflows/pages.yml) passes DOCS_BASE through.
// ---------------------------------------------------------------------------
const base = process.env.DOCS_BASE ?? "/ROCprofGUI/";

export default defineConfig({
  base,
  lang: "en-US",
  title: "rocprof studio",
  description:
    "Documentation for rocprof-studio — a desktop and headless tool for inspecting ROCm GPU profiling captures.",
  lastUpdated: true,
  cleanUrls: true,
  ignoreDeadLinks: false,
  head: [
    ["meta", { name: "theme-color", content: "#111318" }],
    ["link", { rel: "icon", type: "image/svg+xml", href: `${base}logo.svg` }],
  ],
  themeConfig: {
    logo: "/logo.svg",
    nav: [
      { text: "Home", link: "/" },
      { text: "Get started", link: "/installation" },
      { text: "MCP", link: "/mcp" },
      { text: "Views", link: "/views" },
      { text: "Limits", link: "/limits" },
    ],
    sidebar: [
      {
        text: "Overview",
        items: [
          { text: "What rocprof studio is", link: "/" },
          { text: "Installation", link: "/installation" },
          { text: "Running & modes", link: "/running" },
        ],
      },
      {
        text: "Using it",
        items: [
          { text: "Input formats", link: "/input-formats" },
          { text: "UI tour", link: "/ui-tour" },
          { text: "Views reference", link: "/views" },
          { text: "Projects", link: "/projects" },
          { text: "Export & evidence", link: "/export" },
        ],
      },
      {
        text: "Automation & profiling",
        items: [
          { text: "MCP mode", link: "/mcp" },
          { text: "Kernel tuning", link: "/kernel-tuning" },
          { text: "Shader tuning (Vulkan)", link: "/shader-tuning" },
          { text: "RADV / Vulkan profiling", link: "/radv" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "Limits & caveats", link: "/limits" },
          { text: "Architecture", link: "/architecture" },
          { text: "Validation & parity", link: "/validation" },
          { text: "Troubleshooting / FAQ", link: "/troubleshooting" },
          { text: "Changelog", link: "/changelog" },
        ],
      },
    ],
    outline: { level: [2, 3] },
    search: { provider: "local" },
    socialLinks: [],
    footer: {
      message:
        "This site documents rocprof-studio without changing it. Screenshots of the synthetic example are labelled as such and are not GPU measurements.",
    },
  },
});
