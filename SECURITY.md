# Security Policy

smugmap is an `LD_PRELOAD` library that reads AWS credentials from the
environment and signs S3 requests. A bug in this code can leak credentials
or serve attacker-controlled bytes into a running process. Please report
security issues privately.

## Reporting

Open a private [security advisory](../../security/advisories/new) on
GitHub. Include:

- A description of the issue and its impact.
- Reproduction steps or a proof-of-concept, if you have one.
- The commit SHA or release version affected.

Do **not** open a public issue for security bugs. Expect an
acknowledgement within a few days.

## Scope

In scope:

- Credential leakage (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
  `AWS_SESSION_TOKEN`, presigned URLs) via logs, error messages, or
  memory disclosure.
- SigV4 signing correctness (canonical request, header injection).
- Any path that causes smugmap to serve bytes from a URL other than the
  one configured for a given file pattern.
- Memory safety issues in the `unsafe` interposition code.

Out of scope:

- Denial of service via misconfigured URLs or unreachable endpoints.
- Users granting the config file world-readable permissions.
