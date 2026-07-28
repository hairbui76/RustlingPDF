# Document-signing migration design

## Status

**A constrained Rust signing route is enabled; it is not production-PAdES
complete.** Existing Rust signature verification/removal does not authorize
other signing sources. `POST /api/v1/security/cert-sign` accepts
`certType=PEM`, `PKCS12`, `PFX`, `JKS`, desktop-loopback `PKCS11`,
or Windows-desktop `WINDOWS_STORE`. PEM accepts plain/encrypted PKCS#8
and traditional RSA/P-256/P-384 PEM keys with a PEM X.509 chain or one DER
certificate. PKCS#12/PFX is parsed in memory with strict key-to-certificate
matching and supports optional alias selection. JKS v1/v2 stores are
authenticated before bounded parsing and support their legacy Oracle
`KeyProtector` entries. The route produces detached CMS in an incremental
revision and supports either an invisible field or a visible page widget with
a self-contained vector appearance. PKCS#11 keys remain opaque in a serialized,
request-scoped authenticated token session. Windows-store CMS is produced by a
bounded in-memory PowerShell/.NET bridge that keeps CSP/CNG keys in their provider. The managed
server certificate (`certType=SERVER`) was removed together with server-side state; signing
material is always caller-supplied or desktop hardware.
The `rustling_processing::signing_key` boundary keeps request-private key bytes
opaque and zeroized where supported. The route lacks policy validation, a
public compatibility suite, and security review, so it must not be represented
as production-ready PAdES support.

**2026-07-23 AI-assisted review pass (not a substitute for an independent
review):** an adversarially-verified pass against this document's own
non-negotiable constraints found and fixed 6 concrete issues, each with a
regression test. **Critical:** `covers_entire_document` in
`pdf_signature_validation.rs` was computed once as a document-wide flag from
the largest `/ByteRange` among *any* signature-like object, then applied to
every signature's result; an attacker-supplied decoy object with a fabricated
`/ByteRange`/`/Contents` pair (no valid CMS needed) could spoof a genuinely
tampered, appended-to document as fully covered. It is now computed strictly
per-signature from that signature's own byte range. **High:**
`pdf_incremental_signature.rs` located the real signature's `/Contents`
placeholder and `/ByteRange` insertion point by raw byte-scanning from the
start of the whole appended revision, so a decoy `/Contents` key added to the
input PDF's own Catalog (which always serializes at a lower object ID than the
freshly-allocated signature object) could be matched first and hijack the
placeholder; it now locates the offset via the just-written revision's own
PDF-parsed cross-reference table instead of string matching. **High:** the
`/Sig/Name` dictionary entry (later read back out as `signerName` by this
module's own validation endpoint) was populated from the raw client-supplied
`name` form field with no cross-check against the actual signing certificate;
it now reuses the same certificate-CN-preferred value already used for the
visible appearance, so a signer cannot claim a false identity in a
technically-valid signature. **High:** `hardware_signing.rs`'s desktop gate
(`ensure_desktop`/`is_desktop`) checked only process-wide environment
variables, never the calling peer, so any caller reaching a desktop-mode
runtime over the network could enumerate/use hardware certificates as the
bundled desktop UI; it now additionally requires the request's peer address to
be loopback (`ConnectInfo<SocketAddr>` threaded through the two discovery
routes and `cert_sign_pdf`). **Medium:** traditional (non-PKCS#8) PEM parsing
held decoded key bytes and their base64 text in plain `Vec<u8>`/`String`
buffers before wrapping them as the zeroizing `SigningSecret`; both are now
zeroizing from the moment they are decoded. **Medium:** an invalid-UTF-8
PKCS#11 PIN was copied into a plain, unzeroized `Vec<u8>` via
`String::from_utf8` before validation failed; it is now validated by borrowing
(`str::from_utf8`) with no unzeroized copy on the failure path. **Low
(previously missing, now implemented):** PKCS#11 login had no attempt limit,
letting an unbounded number of PIN guesses reach a token through this endpoint
regardless of what the token/driver itself enforces; it now locks out further
attempts against a given `(library, slot)` for 5 minutes after 5 consecutive
failures. (A further low-severity finding concerned the since-removed collaborative
workflow-signing routes and is moot.) None of this changes the status above:
an independent review is still required before this route can be represented
as production-ready PAdES support.

## Java baseline

Java's `POST /api/v1/security/cert-sign` accepts a PDF with PEM + certificate,
PKCS#12/PFX, JKS, managed server certificate, Windows certificate store, or a
PKCS#11 token. It emits detached CMS signatures with optional visible
appearances. Hardware discovery/signing is desktop-and-loopback-only. The Rust
port deliberately drops the managed server certificate and the owner/team
signing workflows: both depended on server-side state, which this product
does not keep.

## Non-negotiable constraints

- Private-key bytes, passwords, and PINs are request-lifetime secrets: zeroize
  where supported; never write them to temp files, responses, logs, metrics,
  traces, audits, or plaintext storage.
- Implement valid incremental PDF updates with a correct `/ByteRange`, detached
  CMS/PAdES signature, and safe appearance; preserve prior signatures and reject
  PDFs that cannot be interpreted safely.
- Certificate chain/key usage, algorithms, digests, timestamps, revocation, and
  appearance policy are server configuration, not client parameters.
- Windows/PKCS#11 requires local desktop loopback plus provider
  allow-lists and PIN attempt limits.

## Delivery and verification gate

1. **Done as a foundation:** source-independent typed `SigningKey` boundary that
   never exposes parsed key bytes.
2. **In progress:** the constrained HTTP route accepts plain/encrypted PKCS#8,
   traditional RSA/P-256/P-384 PEM, in-memory PKCS#12/PFX, and JKS v1/v2. It
   verifies the signing certificate matches the key, bounds signing material
   to 8 MiB, and produces detached CMS. Unit and endpoint tests cover correct
   and incorrect passwords, alias selection, malformed stores, and CMS
   verification. Key-strength/key-usage/chain validation and less common
   legacy PEM ciphers/curves remain required for production policy parity.
3. **In progress:** the route writes an invisible or visible incremental signature field,
   computes a fixed `/ByteRange`, and inserts detached CMS without changing
   signed bytes. Its endpoint test reconstructs that byte range and verifies
   the returned CMS; OpenSSL 3.5 also verifies the core writer. Validate final
   PDFs with Java and Acrobat fixtures before calling it `PAdES`.
4. **Visible appearance slice done:** a one-based page number selects a printed
   widget with a bounded signer/date/reason appearance and optional vector
   mark. Add RFC-3161 signature timestamps with hostname/TLS/nonce and
   bounded-response policy.
5. **PKCS#11 parity slice implemented:** allowlisted driver, explicit slot/key
   selection, zeroizing PIN, mechanism capability checks, opaque key handle,
   request-scoped login/logout, RSA and P-256/P-384 ECDSA CMS signing. A live
   SoftHSM/token matrix and security review remain gates.
6. **Windows-store parity slice implemented:** exact CurrentUser thumbprint
   selection, desktop-loopback gate, SHA-256 detached CMS with signed attributes,
   CSP/CNG provider ownership, bounded anonymous pipes, generic failure output,
   and an opt-in live endpoint verification test. Expand the hardware matrix
   before production approval.
7. **Managed server-certificate slice removed:** the service, its
   administration routes, and `certType=SERVER` were deleted with server-side
   state (maintainer decision 2026-07-28).

Every source must test bad secrets, weak/expired/invalid chains, malformed PDFs,
existing signatures, visual/invisible forms, timestamp failure, and redaction of
secrets. The currently exposed constrained route is a parity slice, not evidence
that this suite or security review has passed.
