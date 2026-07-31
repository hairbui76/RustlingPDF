# Rust operation-catalog generator

`rustling-operation-catalog` translates the committed OpenAPI document into the
self-contained JSON Schema catalog compiled into `rustling-ai-engine`.

From the repository root:

```shell
task engine:tool-models
task engine:tool-models:check
```

The generator keeps the established agent-operation boundary:
only non-parameterized `POST` paths below `general`, `misc`, `security`, and
`convert` are candidates; interactive, introspection, certificate-signing, and
secondary-upload routes remain excluded. File transport fields are removed,
request names are normalized to stable camel-case aliases, and referenced
component schemas are copied transitively so the result has no runtime
dependency on OpenAPI.
