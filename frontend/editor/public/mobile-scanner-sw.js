/* RustlingPDF scanner shell: static/runtime cache only, never API responses. */
"use strict";

const CACHE_NAME = "rustlingpdf-mobile-scanner-v1";
const scopeUrl = new URL(self.registration.scope);
const scannerUrl = new URL("mobile-scanner", scopeUrl).toString();
const initialAssets = [
  scannerUrl,
  new URL("mobile-scanner-manifest.json", scopeUrl).toString(),
  new URL("modern-logo/logo192.png", scopeUrl).toString(),
  new URL("modern-logo/logo512.png", scopeUrl).toString(),
  new URL("vendor/jscanify/opencv.js", scopeUrl).toString(),
  new URL("vendor/jscanify/jscanify.js", scopeUrl).toString(),
];

const isCacheable = (url) =>
  url.origin === scopeUrl.origin &&
  url.pathname.startsWith(scopeUrl.pathname) &&
  !url.pathname.includes("/api/");

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) =>
        Promise.allSettled(
          initialAssets.map((url) =>
            fetch(url).then((response) => {
              if (response.ok) return cache.put(url, response);
              return undefined;
            }),
          ),
        ),
      )
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) =>
        Promise.all(
          names
            .filter(
              (name) =>
                name.startsWith("rustlingpdf-mobile-scanner-") &&
                name !== CACHE_NAME,
            )
            .map((name) => caches.delete(name)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("message", (event) => {
  if (event.data?.type !== "CACHE_URLS" || !Array.isArray(event.data.urls)) {
    return;
  }
  const targets = event.data.urls
    .map((rawUrl) => new URL(rawUrl, scopeUrl))
    .filter(isCacheable);
  const cachePromise = caches.open(CACHE_NAME).then((cache) =>
    Promise.allSettled(
      targets.map((url) => {
        const cacheKey =
          url.pathname.endsWith("/mobile-scanner") ||
          url.pathname.endsWith("/mobile-scanner/")
            ? scannerUrl
            : url.toString();
        return fetch(url.toString()).then((response) => {
          if (!response.ok) {
            throw new Error(`Could not cache ${url.pathname}`);
          }
          return cache.put(cacheKey, response);
        });
      }),
    ),
  );
  event.waitUntil(
    cachePromise.then((results) => {
      const failed = results.filter((result) => result.status === "rejected");
      event.ports[0]?.postMessage({
        type: "CACHE_COMPLETE",
        cached: results.length - failed.length,
        failed: failed.length,
      });
    }),
  );
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  if (!isCacheable(url)) return;

  if (event.request.mode === "navigate") {
    event.respondWith(
      fetch(event.request)
        .then((response) => {
          if (
            response.ok &&
            (url.pathname.endsWith("/mobile-scanner") ||
              url.pathname.endsWith("/mobile-scanner/"))
          ) {
            const copy = response.clone();
            event.waitUntil(
              caches
                .open(CACHE_NAME)
                .then((cache) => cache.put(scannerUrl, copy)),
            );
          }
          return response;
        })
        .catch(() => caches.match(scannerUrl)),
    );
    return;
  }

  event.respondWith(
    caches.match(event.request, { ignoreVary: true }).then(
      (cached) =>
        cached ||
        fetch(event.request).then((response) => {
          if (response.ok) {
            const copy = response.clone();
            event.waitUntil(
              caches
                .open(CACHE_NAME)
                .then((cache) => cache.put(event.request, copy)),
            );
          }
          return response;
        }),
    ),
  );
});
