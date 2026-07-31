# `POST /api/v1/misc/replace-invert-pdf`

Contract for color replacement, inversion, and color-space conversion.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one PDF, required
- `replaceAndInvertOption`: required, one of `HIGH_CONTRAST_COLOR`,
  `CUSTOM_COLOR`, `FULL_INVERSION`, `COLOR_SPACE_CONVERSION`
- `highContrastColorCombination`, `backGroundColor`, `textColor`: accepted but
  only consumed by the text-recoloring modes (see below). The high-contrast
  combination defaults to `WHITE_TEXT_ON_BLACK`. Custom colors accept
  `#RRGGBB`, `0xRRGGBB`, decimal, or leading-zero octal.
- Success returns the original safe filename suffixed with `_inverted.pdf` as
  `application/pdf`.

## Modes

### `FULL_INVERSION`

Every page is rendered with form data and annotations, its colors are
inverted channel-by-channel (`255 - value`), and the page is replaced with a
single RGB image sized to the same PDF page. As with flatten, the result no
longer contains selectable source text. Pages render at `SYSTEM_MAXDPI`
(defaulting to 500 DPI); pixel dimensions and
total pixel count are checked before allocation.

`PDFium` is required. A development runtime without a configured library returns
`501 Not Implemented`; an explicitly configured but broken runtime or a
processing failure returns a server error. Packaged cutover environments install
the pinned native revision.

### `COLOR_SPACE_CONVERSION`

This mode is implemented in-process and does not require an external program.
It converts two layers:

**Content streams.** Page contents and Form XObjects — nested and shared forms are
traversed once each — are rewritten operator by operator. `g`/`G` and `rg`/`RG`
become `k`/`K`, and `sc`/`scn`/`SC`/`SCN` are converted whenever the graphics state
selected `DeviceGray`, `DeviceRGB`, `CalGray`, or `CalRGB` (directly or through a
`/ColorSpace` resource name). `q`/`Q` nesting and separate stroking/non-stroking
state are tracked. The conversion is the device conversion of ISO 32000-1 §10.4 with
full black generation and undercolour removal: gray `v` becomes `0 0 0 (1-v)`, and
RGB becomes `c=1-R, m=1-G, y=1-B, k=min(c,m,y)` with `k` subtracted from the other
three. Colours already in `DeviceCMYK` are emitted unchanged.

**Image XObjects.** 8-bit-per-component images in `DeviceGray`, `DeviceRGB`,
`CalGray`, `CalRGB`, or `ICCBased` with `N` of 1 or 3 are resampled to `DeviceCMYK`
and re-emitted as Flate-compressed 8-bit CMYK. An embedded ICC profile is converted
to sRGB with `moxcms` first, using the same bounded ICC handling as `pdf_json`: an
unusable profile makes the image be skipped rather than guessed at.

**Deliberately left untouched** (a wrong rewrite would corrupt them, and leaving
them is visible and reversible): images used as `/SMask` or `/Mask`, stencil masks,
images carrying a `/Decode` array, non-8-bit depths, `Indexed`/`Separation`/
`DeviceN`/`Lab` colour spaces, four-component `ICCBased` images (already CMYK-like),
images larger than 256 Mi samples, and images behind filters `lopdf` cannot decode
on its own (DCT/JPEG, JPX, CCITT, JBIG2). Shadings (`sh`), pattern colour spaces,
Type3 glyph procedures, and annotation appearance streams keep their original
colours. The result is therefore a document whose device colours are CMYK, not a
certified prepress conversion; unlike Ghostscript's `/prepress` pass it also does
not downsample, re-encode, or restructure anything else.

Failures are limited to reading, decoding, re-encoding, or writing the PDF and
return a server error.

### `HIGH_CONTRAST_COLOR` and `CUSTOM_COLOR`

Ported in pure Rust at the PDF content-stream layer. Each page receives a filled
background rectangle before its existing content. Around every `Tj`, `TJ`,
single-quote, and double-quote text-showing operator Rust sets the requested
non-stroking RGB color and then restores the previous grayscale/RGB/CMYK or
explicit `cs`/`sc[n]` color state. Graphics-state `q`/`Q` nesting is tracked.
Indirect Form XObjects, including nested/shared Forms, are traversed and rewritten
once. Existing strings, font programs, glyph encodings, text matrices, vector
graphics, images, annotations, and selectable text remain intact.

The high-contrast presets map exactly to Java's white/black, black/white,
yellow/black, and green/black pairs. `CUSTOM_COLOR` requires both `textColor`
and `backGroundColor`; missing or invalid values return `400`. Unlike Java's
glyph extraction/redraw path, Rust does not substitute unsupported characters
with `*` because it never re-encodes the original glyph strings. Colorized Type3
glyph programs that set their own fill internally can still override the outer
non-stroking color.

## Verification

HTTP tests cover required-field validation, invalid option/color rejection,
high-contrast/custom page and nested-Form recoloring, preservation of selectable
text, and the `FULL_INVERSION` branch against both the no-native boundary and the
pinned native runtime. `COLOR_SPACE_CONVERSION` is asserted unconditionally, with
no host dependency: one test checks that `rg`/`g`/`RG` are gone and that pure red
became `0 1 1 0`, one converts a `DeviceRGB` image and checks both the resulting
CMYK bytes and that its soft mask kept `DeviceGray`, and one checks an already-CMYK
page round-trips unchanged.
