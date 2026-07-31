# Local-first mobile scanner contract

RustlingPDF ships `/mobile-scanner` as an installable browser scanner. A QR
session is optional: opening the route without `?session=...` starts local mode,
and an unreachable or expired desktop session can be continued locally.
RustlingPDF does not claim a native Android or iOS package.

The scanner supports camera capture plus multi-image file fallback, live edge
detection, four-corner manual perspective correction, rotation, color/clean/
grayscale/black-and-white cleanup, page removal and reordering, and one ordered
multi-page PDF export. To bound browser memory while retaining print-quality
input, a selected photo whose longest edge exceeds 3,000 pixels is resampled to
3,000 pixels before correction; smaller images are unchanged. Edge detection
runs on a separate 240-pixel working image while perspective extraction uses
the bounded full-resolution copy. PDF construction happens in the browser.
Captured image data remains in page memory and is not written to Local Storage,
IndexedDB, an analytics event, or a backend unless the user explicitly chooses
desktop transfer.

After a local PDF export the UI offers OCR and Sign as next tools. The PDF is
downloaded first and the user selects it in the chosen tool; the scanner does
not claim an automatic file handoff that browser sandboxing cannot guarantee.
In a live desktop transfer session the ordered PDF is imported by the existing
desktop/file-manager receiver and is immediately usable by those tools.

## Installable and offline behavior

`mobile-scanner-manifest.json` gives the scanner its own standalone start URL.
`mobile-scanner-sw.js` installs under the configured application base path and,
after one successful online load, caches the scanner navigation response,
same-origin application resources already loaded by the page, OpenCV/jscanify,
and scanner icons. API routes and cross-origin responses are never cached.

An offline reload can therefore capture or select images, correct and clean
them, reorder pages, and export a PDF locally. Camera availability still
depends on the browser retaining permission and exposing `getUserMedia`;
multi-image file input remains the fallback. Desktop transfer is disabled
while offline and can be retried after reconnection.

## Anonymous transfer routes

The following section defines the ephemeral mobile-to-desktop transfer API.

## Routes

- `POST /api/v1/mobile-scanner/create-session/{sessionId}` creates or replaces a
  session and returns `success`, `sessionId`, `createdAt`, `expiresAt`, and
  `timeoutMs`.
- `GET /api/v1/mobile-scanner/validate-session/{sessionId}` returns the same
  session information with `valid: true`, or `404` with
  `{ "valid": false, "error": "Session not found or expired" }`.
- `POST /api/v1/mobile-scanner/upload/{sessionId}` accepts multipart `files`.
  It returns `success`, `sessionId`, `filesUploaded`, and the established
  success message. Uploading creates a missing valid session.
  The current scanner sends one locally created, ordered PDF; the compatibility
  route continues to accept multiple files for older clients.
- `GET /api/v1/mobile-scanner/files/{sessionId}` returns `sessionId`, `count`,
  and file entries containing `filename`, `size`, and `contentType`. A missing
  session has an empty list.
- `GET /api/v1/mobile-scanner/download/{sessionId}/{filename}` serves the file
  as an attachment and removes it immediately after reading. The service deletes
  the session after its last file has been downloaded.
- `DELETE /api/v1/mobile-scanner/session/{sessionId}` deletes the session and
  returns the legacy success body; deleting a missing session remains successful.

## Safety and lifetime

Session IDs accept only ASCII letters, digits, and hyphens. Upload file names
are reduced to Java's `[a-zA-Z0-9._-]` safe set and duplicate names receive a
numeric suffix. Download rejects empty names, parent references, and path
separators. Files live in a private `TempDir`, never in a user-selected path.

Sessions use a ten-minute inactivity timeout. Every create, validation, upload,
list, or download refreshes activity; an expired session is removed on its next
access. The temporary workspace is removed when the Rust process exits. This is
process-local state, so a restart invalidates outstanding QR sessions.

The service worker cache contains only application code and static assets.
Transfer documents still use the private request/session `TempDir` below and
retain its deletion semantics; they are never placed in an offline cache.

`system.enableMobileScanner` (or `SYSTEM_ENABLEMOBILESCANNER`) defaults to
`true`. When disabled, all routes except download return the Java JSON `403`
feature-disabled response; download returns a bare `403`.

## Verification

The HTTP integration test creates a session, streams a multipart file, checks
name sanitisation and attachment delivery, and confirms that the final download
removes the session. It also covers invalid session IDs and feature disablement.
Frontend proof covers local launch without a session, expired-session fallback,
stable page reordering, bounded perspective points, QR URL generation, and
construction/reopening of a two-page PDF. The production build must contain the
scanner manifest, worker, OpenCV, and jscanify assets; the worker is syntax
checked independently.
