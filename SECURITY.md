# Security Policy

## Supported versions

Only the latest release on `main` receives security fixes.

| Version | Supported |
|---------|-----------|
| `main` (latest) | ✅ |
| Older tags | ❌ |

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately via [GitHub Security Advisories](https://github.com/chucksentertainment-hash/COMEBACKHERE-contracts/security/advisories/new).

Include as much of the following as possible:

- Description of the vulnerability and its potential impact
- Affected contract(s): `invoice`, `treasury`, and/or `compliance`
- Steps to reproduce or a proof-of-concept
- Suggested fix (optional)

## Response timeline

| Milestone | Target |
|-----------|--------|
| Acknowledgement | 2 business days |
| Triage and severity assessment | 5 business days |
| Fix or mitigation | 14 business days (critical), 30 business days (other) |
| Public disclosure | Coordinated with reporter after fix is released |

## Scope

In-scope:
- Logic errors in contract state machines (`invoice`, `treasury`, `compliance`)
- Authorization bypass or privilege escalation
- Integer overflow/underflow in payment or settlement math
- Reentrancy or cross-contract call vulnerabilities

Out-of-scope:
- Stellar/Soroban network-level issues (report to [Stellar Bug Bounty](https://www.stellar.org/bug-bounty-program))
- Issues in dependencies outside this repository
- Theoretical vulnerabilities with no practical exploit path

## Disclosure policy

We follow coordinated disclosure. We ask that you give us reasonable time to fix the issue before any public disclosure. We will credit reporters in the release notes unless anonymity is requested.
