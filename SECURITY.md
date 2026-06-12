# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in `rvtest`, please do **not**
open a public issue.  Instead, send a private report to the maintainer
via one of the following channels:

- **Email:** reasvyn@gmail.com
- **GitHub Security Advisory:** https://github.com/reasvyn/rvtest/security/advisories/new

You should receive a response within **48 hours**.  If you do not
hear back, follow up via email to ensure the message was received.

## What to Include

- A clear description of the vulnerability
- Steps to reproduce (proof of concept)
- Potential impact
- Any suggested fix (if available)

## Scope

The following are considered in scope for security reports:

- Code execution vulnerabilities in `rvtest` itself
- Unsafe code (`#[deny(unsafe)]` is enforced; any `unsafe` block
  should be auditable)
- Dependency vulnerabilities with active exploits

Out of scope:

- Theoretical vulnerabilities with no practical exploit
- `cargo test` output injection (this is expected behaviour)
- Missing features (open a regular issue for feature requests)

## Policy

- We will acknowledge receipt within 48 hours
- We will provide an estimated timeline for a fix
- We will credit the reporter in the release notes (unless
  anonymity is requested)
- We will publish a security advisory after the fix is released

## Supported Versions

| Version | Supported |
|---|---|
| 0.x | ✅ |
