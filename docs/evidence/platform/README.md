# Platform evidence

Store one immutable `membrane.platform-acceptance.v1` receipt per signed artifact & clean VM. Receipt must bind exact commit, release generation, version, platform, artifact name, SHA-256, trust outcomes, no-bypass result, & full lifecycle outcomes.

Do not treat synthetic tests or `source-ready` receipts as Mac/Windows artifact acceptance. Redact machine identifiers when they contain personal or credential data.
