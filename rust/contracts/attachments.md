# `POST /api/v1/misc/*attachment*`

Rust compatibility contract for the five routes in `AttachmentController`.

## Routes

- `add-attachments`: accepts one `fileInput` and one or more `attachments`;
  returns `<base>_with_attachments.pdf`
- `list-attachments`: returns JSON objects with `filename`, `size`,
  `contentType`, `description`, `creationDate`, and `modificationDate`
- `extract-attachments`: returns `<base>_attachments.zip`
- `rename-attachment`: accepts `attachmentName` and `newName`; returns
  `<base>_attachment_renamed.pdf`
- `delete-attachment`: accepts `attachmentName`; returns
  `<base>_attachment_deleted.pdf`

The Rust implementation reads recursive embedded-file name trees, prefers
Unicode file specifications, and flattens the tree on mutation like the Java
service. Added streams receive size, content type, description, creation and
modification dates, plus the `UseAttachments` viewer preferences. Extraction
sanitizes paths, uniquifies duplicate names, and enforces the Java 50 MiB per
attachment / 200 MiB total limits.

## Removed: `convertToPdfA3b`

Java, and this port until Ghostscript was removed for its
AGPL-3.0-or-commercial licence, accepted `convertToPdfA3b`. It ran the shared
Ghostscript PDF/A-3b converter over the input, then attached the files with
`AFRelationship=Unspecified` and the catalog `AF` array, and named the response
`<base>_with_attachments_PDFA-3b.pdf`.

PDF/A conversion no longer exists in this service. The field is therefore
**rejected**, not ignored:

- `convertToPdfA3b=true` returns `400 Bad Request` with
  `convertToPdfA3b is no longer supported: PDF/A conversion was removed from this
  service`.
- `convertToPdfA3b=false` is accepted and behaves exactly like omitting it.

Returning a plain, non-archival PDF for a request that explicitly asked for an
archival conversion would be a silent under-delivery, which is why the flag errors
instead. The field is also gone from `SwaggerDoc.json` and from the frontend.

## Remaining cutover boundary

Date values in `list-attachments` currently use decoded PDF date strings rather than
Java's locale-dependent `Date.toString()` representation.

## Verification

An endpoint round trip adds an attachment, verifies its catalog and JSON metadata,
extracts and reads its ZIP payload, renames it, deletes it, and confirms the final
list is empty. Separate assertions cover required attachments and both `convertToPdfA3b`
values: `true` must be refused with the explicit message, `false` must produce the
ordinary `<base>_with_attachments.pdf`.
