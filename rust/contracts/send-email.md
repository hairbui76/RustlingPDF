# SMTP email with attachment

Rust compatibility contract for the conditional Java `EmailController` route.

## Route and configuration

`POST /api/v1/general/send-email` is mounted only when `mail.enabled` (or
`MAIL_ENABLED`) is true. The SMTP relay reads the existing `mail.host`, `port`,
`username`, `password`, `from`, `startTlsEnable`, `startTlsRequired`,
`sslEnable`, `sslTrust`, and `sslCheckServerIdentity` keys, with matching
all-caps environment overrides.

The multipart request accepts `to`, `subject`, `body`, and one required
`fileInput`. It emits an HTML MIME body plus the attachment and returns the
legacy plain-text `Email sent successfully` response after the relay accepts the
message. Missing required fields, invalid addresses, malformed MIME metadata,
relay errors, and incomplete configuration fail without logging credentials or
email contents. Text fields, filenames, and attachments have explicit bounds
in addition to the server-wide upload limit.

The route participates in the generic `?async=true` job wrapper and can be used
by the in-process pipeline dispatcher when enabled.

## TLS policy

Implicit TLS, required STARTTLS, opportunistic STARTTLS, and explicitly
configured plaintext SMTP are supported. Rust validates the relay certificate
and hostname against the standard WebPKI public roots. The Java implementation
defaults `sslTrust` to `*` and permits disabled hostname verification; Rust
rejects those insecure overrides instead of silently recreating a
man-in-the-middle-prone configuration.

## Verification

`tests/send_email_endpoint.rs` proves the conditional attachment route and TLS
policy.
