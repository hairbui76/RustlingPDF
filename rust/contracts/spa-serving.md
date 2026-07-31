# SPA serving contract

The processing binary can serve the built Vite application and its static
assets from one origin.

## Activation

SPA serving is enabled when `RUSTLING_FRONTEND_DIST` or
`system.frontendDist` points to a built `dist/` directory. Without that
setting, the fallback is not installed and unmatched requests return `404`.

The fallback is attached after all API routes, so it cannot shadow `/api/**`.
Only `GET` and `HEAD` requests are served.

## Index and client routes

The index is resolved once at startup from
`customFiles/static/index.html`, then `<dist>/index.html`, and finally the
embedded lightweight fallback. The loader replaces `%BASE_URL%`, pins
`<base href="/">`, and injects `window.RUSTLING_PDF_API_BASE_URL`.

Single- and two-segment client routes without dots receive the SPA index unless
their first segment is reserved for API/static assets or a removed account,
administration, or sharing surface. Reserved and deeper paths use static-file
lookup and return `404` when no file exists.

`/mobile-scanner` serves the SPA by default. In desktop mode it prefers
`mobile-upload.html` when that file is present, allowing a phone to load the
self-contained upload page outside the desktop webview.

## Static files

Path decoding rejects traversal, encoded separators, NUL bytes, empty
segments, drive/stream separators, and symlink escapes. Precompressed Brotli
or gzip variants are selected from `Accept-Encoding` when available.

Cache policy:

- service-worker and PWA metadata: `no-store`;
- hashed assets, images, and fonts: immutable for one year;
- stable public asset directories: one day with stale-while-revalidate;
- `index.html`: `no-cache, must-revalidate`;
- other files: `no-cache`.

MIME types are derived from the requested extension. `Last-Modified` and
`If-Modified-Since` support conditional responses.

## Verification

Unit tests cover path sanitization, route classification, cache tiers, index
transformation, content types, precompressed negotiation, and desktop mobile
scanner selection. Integration tests cover browser and desktop serving paths.
