# Security Policy

PaddleBoard is an alpha-stage fork of [Zed](https://github.com/zed-industries/zed).

## Reporting a vulnerability

Please report security vulnerabilities **privately** through GitHub's
[private vulnerability reporting](https://github.com/paddleboarddev/paddleboard/security/advisories/new).
**Do not open a public issue** for security problems.

PaddleBoard is alpha software maintained by a small team, so responses are best-effort —
we aim to acknowledge reports within a few days.

## Scope

- **PaddleBoard-specific code** is the primary concern here: the `paddleboard_*` crates and
  any change to a shared file tagged `// PaddleBoard:`.
- Vulnerabilities in **inherited upstream Zed code** are best reported to
  [zed-industries/zed](https://github.com/zed-industries/zed/security) so the fix reaches
  everyone; PaddleBoard picks it up on the next weekly upstream merge.

## Supported versions

Only the latest release is supported. PaddleBoard is pre-1.0 and moves quickly, so please
make sure you can reproduce on a current build before reporting.
