# `POST /api/v1/convert/ebook/pdf`

Current contract for eBook-to-PDF conversion.

## Request and response

- Content type: `multipart/form-data`
- `fileInput`: one `.epub`, `.mobi`, `.azw3`, `.fb2`, `.txt`, or `.docx` file, required
- `embedAllFonts`: boolean, default `false`
- `includeTableOfContents`: boolean, default `false`
- `includePageNumbers`: boolean, default `false`
- `optimizeForEbook`: boolean, default `false`
- Success returns `<base>_convertedToPDF.pdf` (`application/pdf`).

## Behavior

Rust invokes Calibre with the same rendering flags as Java:

```
ebook-convert <input> <output.pdf> [--embed-all-fonts] [--pdf-add-toc] [--pdf-page-numbers]
```

It resolves the executable from `RUSTLING_PROCESSING_EBOOK_CONVERT_COMMAND` when
set, otherwise `ebook-convert` / `ebook-convert.exe` on `PATH`. The result must be
a non-empty PDF before it is returned.

`optimizeForEbook` is still accepted for wire compatibility but no longer changes
the output. Java ran a best-effort Ghostscript `-dPDFSETTINGS=/ebook` pass over
Calibre's PDF; Ghostscript was removed from this product for its
AGPL-3.0-or-commercial licence. The pass was already best-effort — an unavailable
or failing Ghostscript returned Calibre's PDF unchanged — so the flag now always
takes that path. Responses are byte-identical to the previous no-Ghostscript
behavior.

## Availability and limitations

Unsupported or missing extensions and invalid boolean fields return `400`. If Calibre
is not discoverable the route returns `501 Not Implemented`; an explicitly configured
but broken command, a failed conversion, or invalid output returns `500`.

This route remains gated by Calibre discovery while its own endpoint mapping is
not part of the current shared group manifest.

## Verification

Unit tests cover accepted extensions, rejected extensions, and all Calibre flags. HTTP
tests cover invalid multipart data and a real Calibre conversion when installed
(otherwise the expected response is `501`).
