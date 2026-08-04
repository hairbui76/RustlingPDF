import react from "@vitejs/plugin-react-swc";
import { compression, defineAlgorithm } from "vite-plugin-compression2";
import fs from "node:fs/promises";
import path, { resolve } from "node:path";
import { constants, brotliCompress, gzip } from "node:zlib";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { defineConfig, loadEnv } from "vite";
import type { Connect, PluginOption } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";
import { viteStaticCopy } from "vite-plugin-static-copy";
import { SHIPPED_LATIN_FONT_FILES } from "./src/core/constants/fallbackFonts";
import { DESKTOP_SHIPPED_LOCALES } from "./src/core/constants/desktopLocales";

/**
 * Brand files a build actually serves, each traced to the thing that asks for
 * it: index.html and the two manifests (favicon, logo192, logo512),
 * `useLogoAssets` (logo-tooltip, the horizontal lockup) and `useLogoPath`
 * (the two no-text marks).
 */
const SHIPPED_BRAND_ASSETS = [
  "favicon.ico",
  "logo192.png",
  "logo512.png",
  "logo-tooltip.svg",
  "RustlingPDFLogoHorizontal.png",
  "RustlingPDFLogoNoTextDark.svg",
  "RustlingPDFLogoNoTextLight.svg",
];
import {
  DESKTOP_LOGO_VARIANT,
  LOGO_FOLDER_BY_VARIANT,
} from "./src/core/constants/logo";

const gzipPromise = promisify(gzip);
const brotliPromise = promisify(brotliCompress);
const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * True when this build was started by the Tauri CLI (`beforeBuildCommand` /
 * `beforeDevCommand` in src-tauri/tauri.conf.json).
 *
 * The desktop build runs the SAME vite mode as the web build (`--mode core`),
 * so the mode cannot tell them apart. The Tauri CLI does export a set of
 * `TAURI_ENV_*` variables into the before-command's environment
 * (TAURI_ENV_PLATFORM / _ARCH / _FAMILY / _TARGET_TRIPLE / _PLATFORM_VERSION),
 * which is the signal used here — verified by running `tauri build` with the
 * before-command replaced by an environment dump.
 *
 * `RUSTLING_DESKTOP_BUILD=1` is an explicit escape hatch for anyone driving the
 * frontend build outside the Tauri CLI (e.g. building dist/ separately in CI
 * and handing it to `tauri build --no-bundle`).
 *
 * Only ever used to DROP output the desktop app cannot use. A web build must
 * keep producing exactly what it produced before.
 */
const isDesktopBuild =
  Boolean(process.env.TAURI_ENV_PLATFORM) ||
  process.env.RUSTLING_DESKTOP_BUILD === "1";

/**
 * Delete build output the desktop app can never reach.
 *
 * dist/ is embedded into the Tauri executable (brotli-compressed, one blob per
 * file) and served from memory over the custom protocol — there is no HTTP
 * server, no Accept-Encoding negotiation, and no crawler. Anything that only
 * exists for one of those is dead weight in the installer.
 *
 * Runs after the public dir has been copied and after the OG prerender, so it
 * must be the last plugin in the list.
 */
function pruneDesktopOnlyOutputPlugin(dirs: string[]): PluginOption {
  return {
    name: "prune-desktop-only-output",
    apply: "build" as const,
    async closeBundle() {
      const distDir = path.resolve(__dirname, "dist");
      for (const dir of dirs) {
        await fs.rm(path.join(distDir, dir), { recursive: true, force: true });
      }
      console.log(
        `[prune-desktop-only-output] removed ${dirs.join(", ")} from the desktop bundle`,
      );
    },
  };
}

/**
 * Drop the CJK CMaps a desktop install has already given up on.
 *
 * pdfjs/cmaps is 1.17 MB (951 KB of installer) of predefined CMap tables, and
 * exactly one code path ever loads them: the Compare tool's
 * pixelCompareWorker (the only `cMapUrl` in the tree). 99.7% of the bytes are
 * the CJK registries (Adobe-Japan1/GB1/CNS1/Korea1/KR) — and the desktop
 * build already, deliberately, does not ship CJK fallback fonts (141 MB, see
 * the fonts/latin copy target below), so a CJK document without embedded
 * fonts renders no CJK glyphs anywhere else in the app either. Compare on
 * such a document degrades the affected page and reports through its
 * existing warnings channel instead of silently shipping a megabyte to every
 * install for it.
 *
 * The non-CJK residue (H/V/Roman + LICENSE, ~3 KB) is kept so any
 * non-CJK predefined-CMap document keeps working. Web and Docker keep the
 * full set — an operator there may serve CJK users.
 */
const DESKTOP_KEPT_CMAPS = new Set([
  "H.bcmap",
  "V.bcmap",
  "Roman.bcmap",
  "LICENSE",
]);

function pruneCjkCmapsPlugin(): PluginOption {
  return {
    name: "prune-cjk-cmaps",
    apply: "build" as const,
    async closeBundle() {
      const cmapsDir = path.resolve(__dirname, "dist", "pdfjs", "cmaps");
      let entries: string[];
      try {
        entries = await fs.readdir(cmapsDir);
      } catch {
        return; // no cmaps in this build
      }
      const dropped = entries.filter((file) => !DESKTOP_KEPT_CMAPS.has(file));
      await Promise.all(
        dropped.map((file) =>
          fs.rm(path.join(cmapsDir, file), { force: true }),
        ),
      );
      console.log(
        `[prune-cjk-cmaps] kept ${entries.length - dropped.length}, removed ${dropped.length} CJK CMaps`,
      );
    },
  };
}

/**
 * Drop the translations a desktop installer does not carry.
 *
 * Kept separate from `pruneDesktopOnlyOutputPlugin` because this removes a
 * subset of a directory rather than the directory: `locales/` still ships, with
 * `DESKTOP_SHIPPED_LOCALES` inside it. The same list filters
 * `supportedLanguages` at runtime, so the picker never offers a language whose
 * file was pruned here.
 */
function pruneUnshippedLocalesPlugin(): PluginOption {
  return {
    name: "prune-unshipped-locales",
    apply: "build" as const,
    async closeBundle() {
      const localesDir = path.resolve(__dirname, "dist", "locales");
      let entries: string[];
      try {
        entries = await fs.readdir(localesDir);
      } catch {
        return; // no locales in this build
      }
      const dropped = entries.filter(
        (code) => !DESKTOP_SHIPPED_LOCALES.includes(code as never),
      );
      await Promise.all(
        dropped.map((code) =>
          fs.rm(path.join(localesDir, code), {
            recursive: true,
            force: true,
          }),
        ),
      );
      console.log(
        `[prune-unshipped-locales] kept ${entries.length - dropped.length}, removed ${dropped.length}`,
      );
    },
  };
}

/**
 * Drop the logo variant a desktop install cannot select.
 *
 * `useLogoVariant` resolves `preferences.logoVariant ?? config.logoStyle`, and
 * nothing in the application ever writes that preference — no settings toggle,
 * no menu — so the variant is whatever the backend reports, which defaults to
 * "classic" (`runtime_config.rs`). The modern set is therefore unreachable in a
 * desktop install while costing 1.57 MB of installer.
 *
 * Ten of its eleven files are byte-identical to the classic set anyway; only
 * `Firstpage.png` genuinely differs. Two are kept because `index.html`
 * hard-references those exact paths for the pre-React favicon and manifest,
 * before any of the variant logic has run.
 *
 * Web and Docker builds keep both sets: there `config.logoStyle` is a real
 * deployment setting, and an operator can point it at either.
 */
const DESKTOP_KEEP_FROM_PRUNED_LOGO = ["favicon.ico", "logo192.png"];

function pruneUnusedLogoVariantPlugin(): PluginOption {
  return {
    name: "prune-unused-logo-variant",
    apply: "build" as const,
    async closeBundle() {
      const keptFolder = LOGO_FOLDER_BY_VARIANT[DESKTOP_LOGO_VARIANT];
      const prunedFolder = Object.values(LOGO_FOLDER_BY_VARIANT).find(
        (folder) => folder !== keptFolder,
      );
      if (!prunedFolder) return;
      const dir = path.resolve(__dirname, "dist", prunedFolder);
      let entries: string[];
      try {
        entries = await fs.readdir(dir);
      } catch {
        return;
      }
      const dropped = entries.filter(
        (file) => !DESKTOP_KEEP_FROM_PRUNED_LOGO.includes(file),
      );
      await Promise.all(
        dropped.map((file) =>
          fs.rm(path.join(dir, file), { recursive: true, force: true }),
        ),
      );
      console.log(
        `[prune-unused-logo-variant] removed ${dropped.length} unreachable ${prunedFolder} files`,
      );
    },
  };
}

function compressStaticCopyPlugin(): PluginOption {
  return {
    name: "compress-static-copy",
    apply: "build" as const,
    async closeBundle() {
      const distDir = path.resolve(__dirname, "dist");
      const targets = ["pdfium", "vendor", "pdfjs"];

      const excludedExtensions = [
        ".gz",
        ".br",
        ".png",
        ".jpg",
        ".jpeg",
        ".gif",
        ".webp",
        ".woff",
        ".woff2",
      ];

      async function walkAndCompress(dirOrFile: string) {
        let stat;
        try {
          stat = await fs.stat(dirOrFile);
        } catch {
          return;
        }

        if (stat.isFile()) {
          const ext = path.extname(dirOrFile).toLowerCase();
          if (stat.size >= 1024 && !excludedExtensions.includes(ext)) {
            const content = await fs.readFile(dirOrFile);

            // Gzip (level 9)
            const gzipped = await gzipPromise(content, { level: 9 });
            await fs.writeFile(`${dirOrFile}.gz`, gzipped);

            // Brotli (quality 11)
            const brotlied = await brotliPromise(content, {
              params: {
                [constants.BROTLI_PARAM_QUALITY]: 11,
              },
            });
            await fs.writeFile(`${dirOrFile}.br`, brotlied);
          }
        } else if (stat.isDirectory()) {
          const files = await fs.readdir(dirOrFile);
          for (const file of files) {
            await walkAndCompress(path.join(dirOrFile, file));
          }
        }
      }

      for (const target of targets) {
        await walkAndCompress(path.join(distDir, target));
      }
    },
  };
}

// Bake per-route Open Graph / Twitter Card tags into static HTML at build time.
//
// The SPA sets these client-side for real browsers, but link-unfurling crawlers
// (Slack, Facebook, X, LinkedIn, iMessage, ...) do not run JavaScript. Prerendering
// flat per-route files (e.g. dist/compress.html) means every static host - Cloudflare
// Pages, Docker's bundled static dir, desktop - serves correct previews with NO
// server-side rendering. Cloudflare Pages serves `compress.html` at `/compress`
// automatically (clean URLs), and the Rust backend serves the same file.
//
// Absolute URLs (best for Facebook/X) are used when a canonical base is known:
// VITE_OG_BASE_URL (custom domain) or CF_PAGES_URL (set automatically by Cloudflare
// Pages). Otherwise URLs stay root-relative, which still resolves against whatever
// origin serves the page (correct for self-hosted Docker). Logic lives in
// scripts/og-prerender.mjs so it can be unit-tested without a full build.
function prerenderOgPlugin(): PluginOption {
  const manifestFile = "public/og-metadata.json";
  return {
    name: "prerender-og",
    apply: "build" as const,
    async closeBundle() {
      const { prerenderOg } = await import("./scripts/og-prerender.mjs");
      const ogBase = (
        process.env.VITE_OG_BASE_URL ||
        process.env.CF_PAGES_URL ||
        ""
      ).replace(/\/+$/, "");
      // Absolute deploy base for nested routes' <base href> (matches vite `base`).
      const subpath = (process.env.RUN_SUBPATH || "").replace(/^\/+|\/+$/g, "");
      const baseHref = subpath ? `/${subpath}/` : "/";
      let manifest;
      try {
        manifest = JSON.parse(
          await fs.readFile(path.resolve(__dirname, manifestFile), "utf8"),
        );
      } catch {
        console.warn(
          `[prerender-og] ${manifestFile} missing; skipping OG prerender. ` +
            "Run `node scripts/generate-og-metadata.mjs`.",
        );
        return;
      }
      const distDir = path.resolve(__dirname, "dist");
      const count = await prerenderOg({ distDir, manifest, ogBase, baseHref });
      console.log(
        `[prerender-og] wrote ${count} prerendered route pages` +
          (ogBase
            ? ` (absolute URLs, base=${ogBase})`
            : " (root-relative URLs)"),
      );
    },
  };
}

/**
 * When the app is served under a subpath (RUN_SUBPATH → base like "/app/"), Vite
 * serves index.html at "/app/" and redirects "/" → the base, but a bare "/app"
 * (no trailing slash) 404s. This middleware redirects "/app" → "/app/" so either
 * form loads the app in dev and `vite preview`. Query strings are preserved.
 */
function subpathBareRedirectPlugin(subpath: string): PluginOption {
  const bare = `/${subpath}`;
  const withSlash = `${bare}/`;
  const redirect: Connect.NextHandleFunction = (req, res, next) => {
    const url = req.url ?? "";
    const q = url.indexOf("?");
    const pathname = q === -1 ? url : url.slice(0, q);
    if (pathname === bare) {
      res.statusCode = 301;
      res.setHeader("Location", withSlash + (q === -1 ? "" : url.slice(q)));
      res.end();
      return;
    }
    next();
  };
  return {
    name: "subpath-bare-redirect",
    configureServer(server) {
      server.middlewares.use(redirect);
    },
    configurePreviewServer(server) {
      server.middlewares.use(redirect);
    },
  };
}

export default defineConfig(async ({ mode, command }) => {
  // Dev-only browser-tab label (worktree folder basename) surfaced by the
  // top-level dev tasks so concurrent worktrees have distinguishable tabs.
  // Only injected during `vite` (dev serve) — never baked into a production
  // build — and carries only the folder name, no path/host/user info.
  const devWorktreeLabel =
    command === "serve" ? (process.env.RUSTLING_DEV_LABEL ?? "") : "";
  // Load env files relative to this config (frontend/editor/), regardless of
  // where the build was invoked from. The previous `process.cwd()` worked when
  // this file lived at frontend/, but after the editor was moved under
  // frontend/editor/ the cwd-based lookup would miss editor/.env*.
  const env = loadEnv(mode, import.meta.dirname, "");
  const parentEnv = loadEnv(mode, resolve(import.meta.dirname, ".."), "");

  const tsconfigProject = "./tsconfig.core.vite.json";

  // Subpath the app is served under (base becomes "/<runSubpath>/"). Empty = root.
  const runSubpath = (env.RUN_SUBPATH || "").replace(/^\/+|\/+$/g, "");

  // Backend proxy target: default localhost:8080. Override via BACKEND_URL env var
  // so the top-level dev launcher can wire a dynamically-assigned backend port.
  const backendUrl = process.env.BACKEND_URL || "http://localhost:8080";
  // Allow host header checks to be configured via env so LAN/reverse-proxy
  // dev setups don't require editing this file for each machine.
  const allowedHostsRaw =
    process.env.FRONTEND_ALLOWED_HOSTS ||
    env.FRONTEND_ALLOWED_HOSTS ||
    parentEnv.FRONTEND_ALLOWED_HOSTS ||
    "";
  const allowedHosts = allowedHostsRaw
    .split(",")
    .map((host) => host.trim())
    .filter(Boolean);
  const backendProxy = {
    target: backendUrl,
    changeOrigin: true,
    secure: false,
    xfwd: true,
  };

  // Shared between `vite` (dev) and `vite preview` (production-build serve, used
  // in CI/E2E) so the live test suite still resolves /api → :8080.
  const backendProxyConfig = {
    "/api": backendProxy,
    "/oauth2": backendProxy,
    "/saml2": backendProxy,
    "/login/oauth2": backendProxy,
    "/login/saml2": backendProxy,
    "/swagger-ui": backendProxy,
    "/v1/api-docs": backendProxy,
  };

  return {
    define: {
      __DEV_WORKTREE_LABEL__: JSON.stringify(devWorktreeLabel),
    },
    plugins: [
      react(),
      ...(runSubpath ? [subpathBareRedirectPlugin(runSubpath)] : []),
      tsconfigPaths({
        projects: [tsconfigProject],
      }),
      // Pre-compressed siblings exist so a static HTTP server (Docker, the Rust
      // backend, Cloudflare Pages) can answer `Accept-Encoding` without
      // compressing on every request. The desktop build has no HTTP layer -
      // Tauri embeds dist/ in the executable and hands the raw bytes to the
      // webview - so nothing ever opens a .gz/.br there and they are pure
      // installer weight. Web builds must keep emitting them.
      ...(isDesktopBuild
        ? []
        : [
            compression({
              threshold: 1024,
              exclude: [/\.(png|jpg|jpeg|gif|webp|woff|woff2)$/],
              algorithms: [
                defineAlgorithm("gzip", { level: 9 }),
                defineAlgorithm("brotliCompress", {
                  params: {
                    [constants.BROTLI_PARAM_QUALITY]: 11,
                  },
                }),
              ],
            }),
          ]),
      // Set ANALYZE=true to emit dist/stats.html (treemap) alongside the
      // build; rollup-plugin-visualizer is ESM-only so we import dynamically.
      ...(process.env.ANALYZE === "true"
        ? [
            (await import("rollup-plugin-visualizer")).visualizer({
              filename: "dist/stats.html",
              template: "treemap",
              gzipSize: true,
              brotliSize: true,
              emitFile: false,
            }) as PluginOption,
          ]
        : []),
      viteStaticCopy({
        targets: [
          // Dev-server only. `wasmPrecompiler` resolves the WASM two different
          // ways: under `import.meta.env.DEV` it asks for `/pdfium/pdfium.wasm`
          // (this copy, served by this plugin's middleware), and in every
          // production build it uses the hashed asset Vite emits from
          // `@embedpdf/pdfium/pdfium.wasm?url` (dist/assets/pdfium-*.wasm).
          // Copying it into dist/ therefore shipped a second, byte-identical
          // 4.6 MB WASM that no production code path can request - the string
          // "pdfium/pdfium.wasm" does not occur anywhere in the built output.
          // node_modules is hoisted to the workspace root (frontend/), so this
          // path walks up one level from editor/.
          ...(command === "serve"
            ? [
                {
                  src: "../node_modules/@embedpdf/pdfium/dist/pdfium.wasm",
                  dest: "pdfium",
                },
              ]
            : []),
          {
            // Copy jscanify vendor files to dist
            src: "public/vendor/jscanify/*",
            dest: "vendor/jscanify",
          },
          {
            // pdfjs-dist CMap data for CJK / non-latin glyph mapping. Required
            // when rendering PDFs inside workers where the default DOM fetch paths
            // aren't available.
            src: "../node_modules/pdfjs-dist/cmaps/*",
            dest: "pdfjs/cmaps",
          },
          {
            // pdfjs-dist standard font data (Helvetica/Times/etc.) needed so
            // workers can substitute non-embedded base 14 fonts without DOM access.
            src: "../node_modules/pdfjs-dist/standard_fonts/*",
            dest: "pdfjs/standard_fonts",
          },
          {
            // Fallback fonts for PDFs that reference a font without embedding
            // it. These MUST be served same-origin: @embedpdf's default font
            // config points at cdn.jsdelivr.net, which would disclose the
            // reader's IP, User-Agent and the document's script system on
            // document open. See @app/services/pdfiumEngine.
            //
            // Latin covers Cyrillic, Greek and Vietnamese as well. CJK
            // (fonts-jp/kr/sc/tc) is deliberately NOT shipped: it is ~141 MB.
            // Named faces, not a glob: the package carries all 18 static
            // weights (4.55 MB in the installer) and only four are shipped.
            // The list is shared with pdfiumFallbackFonts.ts so the files on
            // disk and the config handed to PDFium cannot drift apart.
            src: SHIPPED_LATIN_FONT_FILES.map(
              (file) => `../node_modules/@embedpdf/fonts-latin/fonts/${file}`,
            ),
            dest: "fonts/latin",
          },
          {
            src: "../node_modules/@embedpdf/fonts-arabic/fonts/*",
            dest: "fonts/arabic",
          },
          {
            src: "../node_modules/@embedpdf/fonts-hebrew/fonts/*",
            dest: "fonts/hebrew",
          },
          {
            // Brand assets live in core; the editor serves them by URL per
            // variant, so copy each set to the /{variant}-logo path its
            // manifests, index.html and useLogoAssets resolve against.
            // Named files, not a glob. The brand folders also hold three text
            // lockups that only Brand.stories.tsx imports (from src, so
            // Storybook is unaffected) and, until now, a Firstpage.png that
            // nothing rendered at all — together 0.9 MB per variant of assets
            // no code path can request. A glob shipped them anyway.
            src: SHIPPED_BRAND_ASSETS.map(
              (file) => `src/core/assets/brand/classic-logo/${file}`,
            ),
            dest: "classic-logo",
          },
          {
            src: SHIPPED_BRAND_ASSETS.map(
              (file) => `src/core/assets/brand/modern-logo/${file}`,
            ),
            dest: "modern-logo",
          },
        ],
      }),
      ...(isDesktopBuild ? [] : [compressStaticCopyPlugin()]),
      prerenderOgPlugin(),
      // og_images are Open Graph link-preview art: they are only ever written
      // into <meta property="og:image"> (see useDocumentMeta / getToolOgImage
      // and the prerendered per-route HTML). No <img>, no CSS, no fetch - a
      // browser does not load og:image, an unfurling crawler does. The desktop
      // app has no shareable URL and nothing crawls it, so the art is 6 MB of
      // installer for a tag nobody reads. The <meta> tags themselves stay, so
      // the web build and the prerender are untouched.
      //
      // vendor/jscanify is OpenCV.js plus the scanner wrapper — 8.6 MB raw,
      // 2.25 MB of installer. It is loaded by exactly one route,
      // /mobile-scanner, which is opened *on a phone* from the QR code the
      // upload modal shows. A desktop install cannot serve that page to a
      // phone: the QR falls back to the app's own origin, which is
      // tauri://localhost (http://tauri.localhost on Windows) and does not
      // resolve off the machine, and the bundled sidecar is never given
      // RUSTLING_FRONTEND_DIST so it serves no static files at all. The route
      // is therefore unreachable in the desktop build and its 8.6 MB of
      // computer vision is dead weight. Web and Docker builds keep it.
      //
      // MUST stay last: it runs in closeBundle, after prerenderOgPlugin.
      ...(isDesktopBuild
        ? [
            // pdfjs/standard_fonts joins the prune list: it is pdf.js's copy
            // of the base-14 substitutes (Liberation + Foxit, 511 KB of
            // installer), and the only `standardFontDataUrl` in the tree is
            // the Compare worker's — every other pdf.js path has always run
            // without it, falling back to metric substitution. The pdfium
            // engine's own substitutes (fonts/latin, the set the product
            // actually curates) keep shipping.
            pruneDesktopOnlyOutputPlugin([
              "og_images",
              "vendor/jscanify",
              "pdfjs/standard_fonts",
            ]),
            pruneCjkCmapsPlugin(),
            pruneUnshippedLocalesPlugin(),
            pruneUnusedLogoVariantPlugin(),
          ]
        : []),
    ],
    server: {
      host: true,
      allowedHosts: allowedHosts.length > 0 ? allowedHosts : undefined,
      // make sure this port matches the devUrl port in tauri.conf.json file
      port: 5173,
      // Tauri expects a fixed port, fail if that port is not available
      strictPort: true,
      watch: {
        // tell vite to ignore watching `src-tauri`
        ignored: ["**/src-tauri/**"],
      },
      // Only use proxy in web mode - Tauri handles backend connections directly
      proxy: backendProxyConfig,
    },
    preview: {
      host: true,
      port: 5173,
      strictPort: true,
      proxy: backendProxyConfig,
    },
    build: {
      target: "esnext",
      rollupOptions: {
        output: {
          manualChunks: {
            "vendor-react": ["react", "react-dom"],
            "pdf-engine": ["@embedpdf/engines", "@embedpdf/pdfium"],
          },
        },
      },
    },
    optimizeDeps: {
      exclude: ["@embedpdf/pdfium"],
    },
    // base: "./" produces relative asset URLs which work when dist/ is served
    // at any path (e.g. Spring Boot bundling the frontend at /). But under
    // `vite preview` for deep SPA routes (e.g. /workflow/sign/<token>), the
    // browser resolves ./assets/X.js relative to the current path → 404, then
    // SPA fallback returns index.html as text/html and React never mounts.
    // VITE_BUILD_FOR_PREVIEW=1 (set by the CI playwright steps) overrides to
    // an absolute base so deep-route asset paths resolve to /assets/...
    // Trailing slash required: it becomes `<base href>`, and browsers resolve
    // relative URLs (manifest.json, favicon) against the base's *directory*.
    base: runSubpath
      ? `/${runSubpath}/`
      : process.env.VITE_BUILD_FOR_PREVIEW === "1"
        ? "/"
        : "./",
  };
});
