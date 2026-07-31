#!/usr/bin/env node

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const repositoryRoot = resolve(dirname(new URL(import.meta.url).pathname), "..");
const frontendRequire = createRequire(
  pathToFileURL(resolve(repositoryRoot, "frontend/package.json")),
);
const React = frontendRequire("react");
const { renderToStaticMarkup } = frontendRequire("react-dom/server");
const { QRCodeSVG } = frontendRequire("qrcode.react");

const output = resolve(
  repositoryRoot,
  "rust/crates/rustling-processing/resources/files/Auto Splitter Divider (with instructions).pdf",
);
const qr = renderToStaticMarkup(
  React.createElement(QRCodeSVG, {
    value: "https://rustlingpdf.com",
    size: 260,
    level: "H",
    includeMargin: true,
    title: "RustlingPDF auto-split divider",
  }),
);
const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>RustlingPDF Auto Split Divider</title>
  <style>
    @page { size: Letter; margin: 0; }
    * { box-sizing: border-box; }
    body {
      width: 8.5in;
      height: 11in;
      margin: 0;
      padding: 0.75in;
      font-family: Arial, Helvetica, sans-serif;
      color: #17202a;
      display: grid;
      place-items: center;
      text-align: center;
    }
    main { max-width: 6.5in; }
    h1 { margin: 0 0 0.18in; font-size: 28pt; }
    .brand { color: #2563eb; }
    p { margin: 0.12in auto; max-width: 5.8in; font-size: 14pt; line-height: 1.45; }
    .qr { margin: 0.28in auto; width: 2.8in; height: 2.8in; }
    .note {
      margin-top: 0.28in;
      padding-top: 0.2in;
      border-top: 2px solid #dbe4f0;
      font-size: 11pt;
      color: #52606d;
    }
  </style>
</head>
<body>
  <main>
    <h1><span class="brand">RustlingPDF</span> Auto Split Divider</h1>
    <p>Place this page between document batches before scanning.</p>
    <div class="qr">${qr}</div>
    <p>Upload the combined PDF to <strong>Auto Split PDF</strong>. RustlingPDF detects this QR code, removes the divider, and returns each batch as a separate PDF.</p>
    <p class="note">Print at 100% scale. Keep the QR code flat, unobstructed, and well lit during scanning.</p>
  </main>
</body>
</html>`;

const temporaryDirectory = mkdtempSync(resolve(tmpdir(), "rustling-divider-"));
const htmlPath = resolve(temporaryDirectory, "divider.html");
writeFileSync(htmlPath, html);

const chrome = process.env.CHROME_BIN || "/usr/bin/google-chrome";
const result = spawnSync(
  chrome,
  [
    "--headless",
    "--no-sandbox",
    "--disable-gpu",
    "--no-pdf-header-footer",
    `--print-to-pdf=${output}`,
    pathToFileURL(htmlPath).href,
  ],
  { encoding: "utf8" },
);
rmSync(temporaryDirectory, { recursive: true, force: true });

if (result.status !== 0) {
  process.stderr.write(result.stderr || result.stdout);
  process.exit(result.status ?? 1);
}
process.stdout.write(`Generated ${output}\n`);
