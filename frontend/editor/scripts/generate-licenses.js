#!/usr/bin/env node

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync, readFileSync } from "node:fs";
import path from "node:path";
import { argv } from "node:process";

const inputIdx = argv.indexOf("--input");
const INPUT_FILE = inputIdx > -1 ? argv[inputIdx + 1] : null;
const POSTPROCESS_ONLY = !!INPUT_FILE;

/**
 * Generate 3rd party licenses for frontend dependencies
 * This script creates a JSON file similar to the Java backend's 3rdPartyLicenses.json
 */

const OUTPUT_FILE = path.join(
  import.meta.dirname,
  "..",
  "src",
  "assets",
  "3rdPartyLicenses.json",
);
// package.json lives at the workspace root (frontend/), not editor/. The
// script is at frontend/editor/scripts/, so walk up two levels.
const PACKAGE_JSON = path.join(import.meta.dirname, "..", "..", "package.json");
const PACKAGE_NAME_PATTERN =
  /^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/i;
// The desktop package ships native commands and their runtime libraries, none
// of which the npm report can see. They belong on the same user-facing license
// screen — especially the copyleft ones: LGPL section 6 / section 4 require the
// recipient to be told these libraries are present and replaceable, and a
// license screen that lists only the Apache-2.0 tools would be misleading.
//
// This is the COMPLETE set of native components the desktop bundle ships across
// all platforms — every command, every statically linked library, and every
// Windows PE-closure DLL listed in THIRD-PARTY-NOTICES.txt (sections 1-7),
// including the copyleft ones (the GCC runtime GPL-3.0 with the Runtime Library
// Exception, and the LGPL GnuTLS/Nettle/libtasn1/libidn2/libunistring/libiconv/
// gettext libraries). Keep it in step with
// rust/scripts/desktop-tools/THIRD-PARTY-NOTICES.txt, which is the authoritative
// version and provenance record and is shipped verbatim inside the bundle at
// resources/tools/licenses/. One entry per project (the append loop below
// dedupes by name); where a project ships at different versions on different
// platforms the version shown is the notice's primary one and the exact
// per-platform versions are in the notice.
const DESKTOP_NATIVE_DEPENDENCIES = [
  {
    name: "qpdf",
    version: "12.3.2",
    licenseType: "Apache-2.0",
    link: "https://github.com/qpdf/qpdf/releases/tag/v12.3.2",
  },
  {
    name: "tesseract-ocr",
    version: "5.5.3",
    licenseType: "Apache-2.0",
    link: "https://github.com/tesseract-ocr/tesseract/releases/tag/5.5.3",
  },
  {
    name: "tessdata-fast-eng",
    version: "4.1.0",
    licenseType: "Apache-2.0",
    link: "https://github.com/tesseract-ocr/tessdata_fast/tree/4.1.0",
  },
  {
    name: "leptonica",
    version: "1.85.0",
    licenseType: "BSD-2-Clause",
    link: "https://github.com/DanBloomberg/leptonica/releases/tag/1.85.0",
  },
  // Shipped as separate, unmodified, dynamically linked files next to the
  // bundled qpdf on Linux (resources/tools/qpdf/lib/). A user may replace any
  // of them with their own build — that is what satisfies LGPL-2.1 section 6
  // and LGPL-3.0 section 4 for dynamic linking.
  {
    name: "gnutls",
    version: "3.7.3",
    licenseType: "LGPL-2.1-or-later",
    link: "https://gnutls.org/",
  },
  {
    name: "nettle",
    version: "3.7.3",
    licenseType: "LGPL-3.0-or-later",
    link: "https://www.lysator.liu.se/~nisse/nettle/",
  },
  {
    name: "libtasn1",
    version: "4.18.0",
    licenseType: "LGPL-2.1-or-later",
    link: "https://www.gnu.org/software/libtasn1/",
  },
  {
    name: "libidn2",
    version: "2.3.2",
    licenseType: "LGPL-3.0-or-later",
    link: "https://www.gnu.org/software/libidn/#libidn2",
  },
  {
    name: "libunistring",
    version: "1.0",
    licenseType: "LGPL-3.0-or-later",
    link: "https://www.gnu.org/software/libunistring/",
  },
  {
    name: "p11-kit",
    version: "0.24.0",
    licenseType: "BSD-3-Clause",
    link: "https://p11-glue.github.io/p11-glue/p11-kit.html",
  },
  {
    name: "libffi",
    version: "3.4.2",
    licenseType: "MIT",
    link: "https://sourceware.org/libffi/",
  },
  {
    name: "libjpeg-turbo",
    version: "2.1.2",
    licenseType: "BSD-3-Clause",
    link: "https://libjpeg-turbo.org/",
  },
  {
    name: "libpng",
    version: "1.6.46",
    licenseType: "libpng-2.0",
    link: "http://www.libpng.org/pub/png/libpng.html",
  },
  {
    name: "zlib",
    version: "1.3.1",
    licenseType: "Zlib",
    link: "https://zlib.net/",
  },
  // --- Statically linked into the Linux/musl and macOS tesseract command
  // (THIRD-PARTY-NOTICES.txt sections 4 and 6). musl and the GCC runtime are
  // the ones the audit found missing; the GCC runtime is copyleft-with-exception
  // and must be surfaced.
  {
    name: "musl",
    version: "1.2.5",
    licenseType: "MIT",
    link: "https://musl.libc.org/",
  },
  {
    name: "gcc-runtime",
    version: "",
    licenseType: "GPL-3.0-or-later WITH GCC-exception-3.1",
    link: "https://gcc.gnu.org/",
  },
  {
    name: "libtiff",
    version: "4.7.0",
    licenseType: "libtiff",
    link: "https://libtiff.gitlab.io/libtiff/",
  },
  // --- Additional libraries shipped only in the Windows PE import closure
  // (THIRD-PARTY-NOTICES.txt section 7). Includes the libiconv / gettext LGPL
  // libraries the audit flagged as missing. JBIG-KIT is deliberately absent: the
  // Windows libtiff-6.dll is RustlingPDF's own JBIG-free rebuild, so the only
  // GPL-2.0 component no longer ships (see THIRD-PARTY-NOTICES.txt §7).
  {
    name: "libiconv",
    version: "",
    licenseType: "LGPL-2.1-or-later",
    link: "https://www.gnu.org/software/libiconv/",
  },
  {
    name: "gettext",
    version: "",
    licenseType: "LGPL-2.1-or-later",
    link: "https://www.gnu.org/software/gettext/",
  },
  {
    name: "libarchive",
    version: "3.8.8",
    licenseType: "BSD-2-Clause",
    link: "https://www.libarchive.org/",
  },
  {
    name: "brotli",
    version: "1.1.0",
    licenseType: "MIT",
    link: "https://github.com/google/brotli",
  },
  {
    name: "bzip2",
    version: "",
    licenseType: "bzip2-1.0.6",
    link: "https://sourceware.org/bzip2/",
  },
  {
    name: "curl",
    version: "8.21.0",
    licenseType: "curl",
    link: "https://curl.se/",
  },
  {
    name: "libdeflate",
    version: "",
    licenseType: "MIT",
    link: "https://github.com/ebiggers/libdeflate",
  },
  {
    name: "expat",
    version: "2.8.2",
    licenseType: "MIT",
    link: "https://libexpat.github.io/",
  },
  {
    name: "giflib",
    version: "6.1.3",
    licenseType: "MIT",
    link: "https://giflib.sourceforge.net/",
  },
  {
    name: "lerc",
    version: "",
    licenseType: "Apache-2.0",
    link: "https://github.com/Esri/lerc",
  },
  {
    name: "lz4",
    version: "1.10.0",
    licenseType: "BSD-2-Clause",
    link: "https://lz4.org/",
  },
  {
    name: "xz",
    version: "5.8.3",
    licenseType: "0BSD",
    link: "https://tukaani.org/xz/",
  },
  {
    name: "openjpeg",
    version: "2.5.4",
    licenseType: "BSD-2-Clause",
    link: "https://www.openjpeg.org/",
  },
  {
    name: "libpsl",
    version: "0.21.5",
    licenseType: "MIT",
    link: "https://github.com/rockdaboot/libpsl",
  },
  {
    name: "libwebp",
    version: "",
    licenseType: "BSD-3-Clause",
    link: "https://developers.google.com/speed/webp",
  },
  {
    name: "libssh2",
    version: "1.11.1",
    licenseType: "BSD-3-Clause",
    link: "https://libssh2.org/",
  },
  {
    name: "zstd",
    version: "1.5.7",
    licenseType: "BSD-3-Clause",
    link: "https://facebook.github.io/zstd/",
  },
  {
    name: "libb2",
    version: "",
    licenseType: "CC0-1.0",
    link: "https://github.com/BLAKE2/libb2",
  },
  {
    name: "winpthreads",
    version: "",
    licenseType: "MIT",
    link: "https://www.mingw-w64.org/",
  },
  {
    name: "msvc-runtime",
    version: "",
    licenseType: "LicenseRef-Microsoft-Visual-C++-Runtime",
    link: "https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files",
  },
];

// Ensure the output directory exists
const outputDir = path.dirname(OUTPUT_FILE);
if (!existsSync(outputDir)) {
  mkdirSync(outputDir, { recursive: true });
}

console.log("🔍 Generating frontend license report...");

try {
  // Safety guard: don't run this script on fork PRs (workflow setzt PR_IS_FORK)
  if (process.env.PR_IS_FORK === "true" && !POSTPROCESS_ONLY) {
    console.error(
      "Fork PR detected: only --input (postprocess-only) mode is allowed.",
    );
    process.exit(2);
  }

  let licenseData;
  // Generate license report using pinned license-checker; disable lifecycle scripts
  if (POSTPROCESS_ONLY) {
    if (!INPUT_FILE || !existsSync(INPUT_FILE)) {
      console.error("❌ --input file missing or not found");
      process.exit(1);
    }
    licenseData = JSON.parse(readFileSync(INPUT_FILE, "utf8"));
  } else {
    const licenseReport = execSync(
      // 'npx --yes license-checker@25.0.1 --production --json',
      "npx --yes license-report --only=prod --output=json",
      {
        encoding: "utf8",
        cwd: path.dirname(PACKAGE_JSON),
        env: { ...process.env, NPM_CONFIG_IGNORE_SCRIPTS: "true" },
      },
    );
    try {
      licenseData = JSON.parse(licenseReport);
    } catch (parseError) {
      console.error("❌ Failed to parse license data:", parseError.message);
      console.error("Raw output:", licenseReport.substring(0, 500) + "...");
      process.exit(1);
    }
  }

  if (!Array.isArray(licenseData)) {
    console.error("❌ Invalid license data structure");
    process.exit(1);
  }

  const existingModuleUrls = loadExistingModuleUrls();

  // Convert license-checker format to array
  const licenseArray = licenseData.map((dep) => {
    let licenseType = dep.licenseType;
    const projectUrl = getProjectUrl(dep.name, dep.link, existingModuleUrls);

    // Handle missing or null licenses
    if (!licenseType || licenseType === null || licenseType === undefined) {
      licenseType = "Unknown";
    }

    // Handle empty string licenses
    if (licenseType === "") {
      licenseType = "Unknown";
    }

    // Handle array licenses (rare but possible)
    if (Array.isArray(licenseType)) {
      licenseType = licenseType.join(" AND ");
    }

    // Handle object licenses (fallback)
    if (typeof licenseType === "object" && licenseType !== null) {
      licenseType = "Unknown";
    }

    if (
      "posthog-js" === dep.name &&
      licenseType.startsWith("SEE LICENSE IN LICENSE")
    ) {
      licenseType =
        "SEE LICENSE IN LICENSE https://github.com/PostHog/posthog-js/blob/main/LICENSE";
    }

    return {
      name: dep.name,
      version:
        dep.installedVersion ||
        dep.definedVersion ||
        dep.remoteVersion ||
        "unknown",
      licenseType: licenseType,
      repository: projectUrl,
      url: projectUrl,
      link: projectUrl,
    };
  });
  // The desktop package also ships native commands that are not visible to
  // the npm report. Keep them on the same user-facing license surface.
  for (const dependency of DESKTOP_NATIVE_DEPENDENCIES) {
    if (!licenseArray.some((existing) => existing.name === dependency.name)) {
      licenseArray.push({
        ...dependency,
        repository: dependency.link,
        url: dependency.link,
      });
    }
  }

  // Transform to match Java backend format
  const transformedData = {
    dependencies: licenseArray.map((dep) => {
      const licenseType = Array.isArray(dep.licenseType)
        ? dep.licenseType.join(", ")
        : dep.licenseType || "Unknown";
      const licenseUrl = getLicenseUrl(licenseType) || dep.link;

      return {
        moduleName: dep.name,
        moduleUrl:
          dep.repository ||
          dep.url ||
          `https://www.npmjs.com/package/${dep.name}`,
        moduleVersion: dep.version,
        moduleLicense: licenseType,
        moduleLicenseUrl: licenseUrl,
      };
    }),
  };

  // Log summary of license types found
  const licenseSummary = licenseArray.reduce((acc, dep) => {
    const license = Array.isArray(dep.licenseType)
      ? dep.licenseType.join(", ")
      : dep.licenseType || "Unknown";
    acc[license] = (acc[license] || 0) + 1;
    return acc;
  }, {});

  console.log("📊 License types found:");
  Object.entries(licenseSummary).forEach(([license, count]) => {
    console.log(`   ${license}: ${count} packages`);
  });

  // Log any complex or unusual license formats for debugging
  const complexLicenses = licenseArray.filter(
    (dep) =>
      dep.licenseType &&
      (dep.licenseType.includes("AND") ||
        dep.licenseType.includes("OR") ||
        dep.licenseType === "Unknown" ||
        dep.licenseType.includes("SEE LICENSE")),
  );

  if (complexLicenses.length > 0) {
    console.log("\n🔍 Complex/Edge case licenses detected:");
    complexLicenses.forEach((dep) => {
      console.log(`   ${dep.name}@${dep.version}: "${dep.licenseType}"`);
    });
  }

  // Check for potentially problematic licenses
  const problematicLicenses = checkLicenseCompatibility(
    licenseSummary,
    licenseArray,
  );
  if (problematicLicenses.length > 0) {
    console.log("\n⚠️  License compatibility warnings:");
    problematicLicenses.forEach((warning) => {
      console.log(`   ${warning.message}`);
    });

    // Write license warnings to a separate file for CI/CD
    const warningsFile = path.join(
      import.meta.dirname,
      "..",
      "src",
      "assets",
      "license-warnings.json",
    );
    writeFileSync(
      warningsFile,
      JSON.stringify(
        {
          warnings: problematicLicenses,
          generated: new Date().toISOString(),
        },
        null,
        2,
      ),
    );
    console.log(`⚠️  License warnings saved to: ${warningsFile}`);
  } else {
    console.log("\n✅ All licenses appear to be corporate-friendly");
  }

  // Write to file
  writeFileSync(OUTPUT_FILE, JSON.stringify(transformedData, null, 2) + "\n");

  console.log(`✅ License report generated successfully!`);
  console.log(`📄 Found ${transformedData.dependencies.length} dependencies`);
  console.log(`💾 Saved to: ${OUTPUT_FILE}`);
} catch (error) {
  console.error("❌ Error generating license report:", error.message);
  process.exit(1);
}

/**
 * Get standard license URLs for common licenses
 */
function getLicenseUrl(licenseType) {
  if (!licenseType || licenseType === "Unknown") return "";

  const explicitLicenseUrl = licenseType.match(/https?:\/\/\S+/)?.[0];
  if (explicitLicenseUrl) return explicitLicenseUrl;

  const licenseUrls = {
    MIT: "https://opensource.org/licenses/MIT",
    "MIT*": "https://opensource.org/licenses/MIT",
    "Apache-2.0": "https://www.apache.org/licenses/LICENSE-2.0",
    "Apache License 2.0": "https://www.apache.org/licenses/LICENSE-2.0",
    "BSD-3-Clause": "https://opensource.org/licenses/BSD-3-Clause",
    "BSD-2-Clause": "https://opensource.org/licenses/BSD-2-Clause",
    BSD: "https://opensource.org/licenses/BSD-3-Clause",
    "GPL-3.0": "https://www.gnu.org/licenses/gpl-3.0.html",
    "GPL-2.0": "https://www.gnu.org/licenses/gpl-2.0.html",
    "LGPL-2.1": "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html",
    "LGPL-3.0": "https://www.gnu.org/licenses/lgpl-3.0.html",
    // The bundled native runtime libraries declare "-or-later" SPDX ids. Without
    // these entries the license screen would fall back to the project homepage
    // instead of linking to the license the library is actually under.
    "LGPL-2.1-or-later":
      "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html",
    "LGPL-3.0-or-later": "https://www.gnu.org/licenses/lgpl-3.0.html",
    "GPL-2.0-or-later": "https://www.gnu.org/licenses/gpl-2.0.html",
    "GPL-3.0-or-later": "https://www.gnu.org/licenses/gpl-3.0.html",
    // Bundled native tools: the GCC runtime carries the Runtime Library
    // Exception, and a few C libraries use their own permissive licences.
    "GPL-3.0-or-later WITH GCC-exception-3.1":
      "https://www.gnu.org/licenses/gcc-exception-3.1.html",
    "0BSD": "https://opensource.org/license/0bsd",
    libtiff: "https://gitlab.com/libtiff/libtiff/-/blob/master/LICENSE.md",
    curl: "https://curl.se/docs/copyright.html",
    "bzip2-1.0.6": "https://sourceware.org/bzip2/",
    "LicenseRef-Microsoft-Visual-C++-Runtime":
      "https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files",
    "libpng-2.0": "http://www.libpng.org/pub/png/src/libpng-LICENSE.txt",
    ISC: "https://opensource.org/licenses/ISC",
    "CC0-1.0": "https://creativecommons.org/publicdomain/zero/1.0/",
    Unlicense: "https://unlicense.org/",
    "MPL-2.0": "https://www.mozilla.org/en-US/MPL/2.0/",
    WTFPL: "http://www.wtfpl.net/",
    Zlib: "https://opensource.org/licenses/Zlib",
    "Artistic-2.0": "https://opensource.org/licenses/Artistic-2.0",
    "EPL-1.0": "https://www.eclipse.org/legal/epl-v10.html",
    "EPL-2.0": "https://www.eclipse.org/legal/epl-2.0/",
    "CDDL-1.0": "https://opensource.org/licenses/CDDL-1.0",
    Ruby: "https://www.ruby-lang.org/en/about/license.txt",
    "Python-2.0": "https://www.python.org/download/releases/2.0/license/",
    "Public Domain": "https://creativecommons.org/publicdomain/zero/1.0/",
    UNLICENSED: "",
  };

  // Try exact match first
  if (licenseUrls[licenseType]) {
    return licenseUrls[licenseType];
  }

  // Try case-insensitive match
  const lowerType = licenseType.toLowerCase();
  for (const [key, url] of Object.entries(licenseUrls)) {
    if (key.toLowerCase() === lowerType) {
      return url;
    }
  }

  // Handle complex SPDX expressions like "(MIT AND Zlib)" or "(MIT OR CC0-1.0)"
  if (licenseType.includes("AND") || licenseType.includes("OR")) {
    // Extract the first license from compound expressions for URL
    const match = licenseType.match(/\(?\s*([A-Za-z0-9\-.]+)/);
    if (match && licenseUrls[match[1]]) {
      return licenseUrls[match[1]];
    }
  }

  // For non-standard licenses, return empty string (will use package link if available)
  return "";
}

function getProjectUrl(packageName, reportedUrl, existingModuleUrls) {
  if (!PACKAGE_NAME_PATTERN.test(packageName)) {
    throw new Error(`Invalid package name: ${packageName}`);
  }

  return (
    normalizeProjectUrl(reportedUrl) ||
    normalizeProjectUrl(existingModuleUrls.get(packageName)) ||
    `https://www.npmjs.com/package/${packageName}`
  );
}

function loadExistingModuleUrls() {
  try {
    const existingReport = JSON.parse(readFileSync(OUTPUT_FILE, "utf8"));
    return new Map(
      (existingReport.dependencies ?? [])
        .filter((dependency) => dependency.moduleName && dependency.moduleUrl)
        .map((dependency) => [dependency.moduleName, dependency.moduleUrl]),
    );
  } catch {
    return new Map();
  }
}

function normalizeProjectUrl(url) {
  if (!url || url === "n/a") return "";

  return url
    .replace(/^git\+/, "")
    .replace(/^git:\/\/github\.com\//, "https://github.com/")
    .replace(/^github:/, "https://github.com/")
    .replace(/\.git$/, "");
}

/**
 * Check for potentially problematic licenses that may not be MIT/corporate compatible
 */
function checkLicenseCompatibility(licenseSummary, licenseArray) {
  const warnings = [];

  // Define problematic license patterns
  const problematicLicenses = {
    // Copyleft licenses
    "GPL-2.0": "Strong copyleft license - requires derivative works to be GPL",
    "GPL-3.0": "Strong copyleft license - requires derivative works to be GPL",
    "LGPL-2.1":
      "Weak copyleft license - may require source disclosure for modifications",
    "LGPL-3.0":
      "Weak copyleft license - may require source disclosure for modifications",
    "AGPL-3.0":
      "Network copyleft license - requires source disclosure for network use",
    "AGPL-1.0":
      "Network copyleft license - requires source disclosure for network use",

    // Other potentially problematic licenses
    WTFPL: "Potentially problematic license - legal uncertainty",
    "CC-BY-SA-4.0":
      "ShareAlike license - requires derivative works to use same license",
    "CC-BY-SA-3.0":
      "ShareAlike license - requires derivative works to use same license",
    "CC-BY-NC-4.0": "Non-commercial license - prohibits commercial use",
    "CC-BY-NC-3.0": "Non-commercial license - prohibits commercial use",
    "OSL-3.0": "Copyleft license - requires derivative works to be OSL",
    "EPL-1.0": "Weak copyleft license - may require source disclosure",
    "EPL-2.0": "Weak copyleft license - may require source disclosure",
    "CDDL-1.0": "Weak copyleft license - may require source disclosure",
    "CDDL-1.1": "Weak copyleft license - may require source disclosure",
    "CPL-1.0": "Weak copyleft license - may require source disclosure",
    "MPL-1.1": "Weak copyleft license - may require source disclosure",
    "EUPL-1.1": "Copyleft license - requires derivative works to be EUPL",
    "EUPL-1.2": "Copyleft license - requires derivative works to be EUPL",
    UNLICENSED: "No license specified - usage rights unclear",
    Unknown: "License not detected - manual review required",
  };

  // Known good licenses (no warnings needed)
  const goodLicenses = new Set([
    "MIT",
    "MIT*",
    "Apache-2.0",
    "Apache License 2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSD",
    "ISC",
    "CC0-1.0",
    "Public Domain",
    "Unlicense",
    "0BSD",
    "BlueOak-1.0.0",
    "Zlib",
    // Permissive licences of the bundled native C libraries — surfaced on the
    // licence screen but not compatibility concerns.
    "libtiff",
    "curl",
    "bzip2-1.0.6",
    "Artistic-2.0",
    "Python-2.0",
    "Ruby",
    "MPL-2.0",
    "CC-BY-4.0",
    "SEE LICENSE IN https://raw.githubusercontent.com/Stirling-Tools/Stirling-PDF/refs/heads/main/proprietary/LICENSE",
    "SEE LICENSE IN LICENSE https://github.com/PostHog/posthog-js/blob/main/LICENSE",
  ]);

  // Helper function to normalize license names for comparison
  function normalizeLicense(license) {
    return license
      .replace(/-or-later$/, "") // Remove -or-later suffix
      .replace(/\+$/, "") // Remove + suffix
      .trim();
  }

  // Check each license type
  Object.entries(licenseSummary).forEach(([license, count]) => {
    // Skip known good licenses
    if (goodLicenses.has(license)) {
      return;
    }

    // Check if this license only affects our own packages
    const affectedPackages = licenseArray.filter((dep) => {
      const depLicense = Array.isArray(dep.licenseType)
        ? dep.licenseType.join(", ")
        : dep.licenseType;
      return depLicense === license;
    });

    const isOnlyOurPackages = affectedPackages.every(
      (dep) =>
        dep.name === "frontend" ||
        dep.name.toLowerCase().includes("stirling-pdf") ||
        dep.name.toLowerCase().includes("stirling_pdf") ||
        dep.name.toLowerCase().includes("stirlingpdf"),
    );

    if (
      isOnlyOurPackages &&
      (license === "UNLICENSED" || license.startsWith("SEE LICENSE IN"))
    ) {
      return; // Skip warnings for our own Stirling-PDF packages
    }

    // Check for compound licenses like "(MIT AND Zlib)" or "(MIT OR CC0-1.0)"
    if (license.includes("AND") || license.includes("OR")) {
      // For OR licenses, check if there's at least one acceptable license option
      if (license.includes("OR")) {
        // Extract license components from OR expression
        const orComponents = license
          .replace(/[()]/g, "") // Remove parentheses
          .split(" OR ")
          .map((component) => component.trim());

        // Check if any component is in the goodLicenses set (with normalization)
        const hasGoodLicense = orComponents.some((component) => {
          const normalized = normalizeLicense(component);
          return goodLicenses.has(component) || goodLicenses.has(normalized);
        });

        if (hasGoodLicense) {
          return; // Skip warning - can use the good license option
        }
      }

      // For AND licenses or OR licenses with no good options, check for problematic components
      const hasProblematicComponent = Object.keys(problematicLicenses).some(
        (problematic) => license.includes(problematic),
      );

      if (hasProblematicComponent) {
        const affectedPackages = licenseArray
          .filter((dep) => {
            const depLicense = Array.isArray(dep.licenseType)
              ? dep.licenseType.join(", ")
              : dep.licenseType;
            return depLicense === license;
          })
          .map((dep) => ({
            name: dep.name,
            version: dep.version,
            url:
              dep.repository ||
              dep.url ||
              `https://www.npmjs.com/package/${dep.name}`,
          }));

        const licenseType = license.includes("AND") ? "AND" : "OR";
        const reason =
          licenseType === "AND"
            ? "Compound license with AND requirement - all components must be compatible"
            : "Compound license with potentially problematic components and no good fallback options";

        warnings.push({
          message: `📋 This PR contains ${count} package${count > 1 ? "s" : ""} with compound license "${license}" - manual review recommended`,
          licenseType: license,
          licenseUrl: "",
          reason: reason,
          packageCount: count,
          affectedDependencies: affectedPackages,
        });
      }
      return;
    }

    // Check for exact matches with problematic licenses
    if (problematicLicenses[license]) {
      const affectedPackages = licenseArray
        .filter((dep) => {
          const depLicense = Array.isArray(dep.licenseType)
            ? dep.licenseType.join(", ")
            : dep.licenseType;
          return depLicense === license;
        })
        .map((dep) => ({
          name: dep.name,
          version: dep.version,
          url:
            dep.repository ||
            dep.url ||
            `https://www.npmjs.com/package/${dep.name}`,
        }));

      const packageList =
        affectedPackages
          .map((pkg) => pkg.name)
          .slice(0, 5)
          .join(", ") +
        (affectedPackages.length > 5
          ? `, and ${affectedPackages.length - 5} more`
          : "");
      const licenseUrl =
        getLicenseUrl(license) || "https://opensource.org/licenses";

      warnings.push({
        message: `⚠️  This PR contains ${count} package${count > 1 ? "s" : ""} with license type [${license}](${licenseUrl}) - ${problematicLicenses[license]}. Affected packages: ${packageList}`,
        licenseType: license,
        licenseUrl: licenseUrl,
        reason: problematicLicenses[license],
        packageCount: count,
        affectedDependencies: affectedPackages,
      });
    } else {
      // Unknown license type - flag for manual review
      const affectedPackages = licenseArray
        .filter((dep) => {
          const depLicense = Array.isArray(dep.licenseType)
            ? dep.licenseType.join(", ")
            : dep.licenseType;
          return depLicense === license;
        })
        .map((dep) => ({
          name: dep.name,
          version: dep.version,
          url:
            dep.repository ||
            dep.url ||
            `https://www.npmjs.com/package/${dep.name}`,
        }));

      warnings.push({
        message: `❓ This PR contains ${count} package${count > 1 ? "s" : ""} with unknown license type "${license}" - manual review required`,
        licenseType: license,
        licenseUrl: "",
        reason: "Unknown license type",
        packageCount: count,
        affectedDependencies: affectedPackages,
      });
    }
  });

  return warnings;
}
