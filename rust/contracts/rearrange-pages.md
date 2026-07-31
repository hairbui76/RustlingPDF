# `POST /api/v1/general/rearrange-pages`

Current contract for rearranging PDF pages.

## Request

- Content type: `multipart/form-data`
- `fileInput`: one PDF file, required
- `pageNumbers`: optional custom one-based order, using the shared page syntax
- `customMode`: optional and case-insensitive
- Supported modes: `CUSTOM`, `REVERSE_ORDER`, `DUPLEX_SORT`, `BOOKLET_SORT`,
  `SIDE_STITCH_BOOKLET_SORT`, `ODD_EVEN_SPLIT`, `REMOVE_FIRST`, `REMOVE_LAST`,
  `REMOVE_FIRST_AND_LAST`, and `DUPLICATE`
- `DUPLICATE` reads its count from `pageNumbers`, defaults invalid or absent
  counts to 2, and enforces Java's `max(100, totalPages * 3)` limit

## Response

- Success: `200 OK`, `Content-Type: application/pdf`
- Download name: the input extension is removed and `_rearranged.pdf` is appended
- Existing page dictionaries are reused for their first occurrence
- Repeated occurrences are cloned into distinct page nodes while sharing their
  underlying content/resources
- Booklet padding and odd-page behavior include repeated last-page slots in
  side-stitch mode

The implementation rewrites the existing page tree in place with `lopdf`; it does
not create a separate document or copy resources between documents.

## Inherited page attributes

Every selected page is re-parented onto the root `/Pages` node, which severs it
from any intermediate `/Pages` node it used to inherit from. Before that
happens, `/Resources`, `/MediaBox`, `/CropBox` and `/Rotate` are materialized
onto each page dictionary itself; attributes a page already declares are left
alone, because they override anything an ancestor offers. A source whose
intermediate node carries `/MediaBox [0 0 595 842] /Rotate 90` therefore keeps
that geometry in the output instead of losing it and falling back to Letter.

This is a deliberate divergence from the Java oracle: PDFBox's
`PDPageTree.add(PDPage)` only rewrites `/Parent`, so the upstream in-place
rearrange drops the same attributes. Matching that would emit structurally
lossy PDFs, so RustlingPDF fixes it instead. Inheritance itself is legal and
common (ISO 32000-1 7.7.3.4).

## Verification

Unit tests cover all predefined order algorithms. Endpoint tests cover custom
order, response headers and filename, duplicate mode, distinct duplicate nodes,
side-stitch padding, and route-specific invalid-mode errors. These tests are
engine-independent and run in both native and fallback quality gates.
