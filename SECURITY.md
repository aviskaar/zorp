# Security Policy

## Reporting a Vulnerability

Use the **Report a vulnerability** button on the Security tab of this repository to file a private security advisory. This is the only supported reporting channel. Do not file public issues for security vulnerabilities.

If you cannot use the private advisory form, email security@aviskaar.com with the subject line "[SECURITY] zorp vulnerability report" and include:
- A description of the vulnerability
- Steps to reproduce or a proof of concept
- The affected versions (if known)
- Any suggested mitigations or fixes

## Supported Versions

Only the latest release on the `main` branch receives security updates. Earlier versions are not patched.

| Version | Supported |
|---------|-----------|
| Latest release (main) | Yes |
| Older releases | No |

We do not maintain long-term support branches. Update to the latest release to receive fixes.

## Response Expectations

- **Acknowledgment**: We will acknowledge receipt within 3 business days.
- **Initial assessment**: We will provide an initial assessment of severity and scope within 10 business days.
- **Status updates**: We will share progress updates at least every 14 days until resolution.
- **Resolution target**: We aim to ship a fix for critical and high severity issues within 30 days of acknowledgment. Medium and low severity issues may take longer depending on complexity.

## Disclosure Process

1. **Private reporting**: The reporter submits details through the private advisory form or email.
2. **Confirmation**: We confirm the vulnerability, assess severity, and assign a tracking identifier.
3. **Development**: We develop and test a fix in a private fork.
4. **Coordinated disclosure**: We agree on a disclosure date with the reporter (default: 90 days from acknowledgment, or sooner if a fix is ready and tested).
5. **Release**: We publish the fix in a new release on `main` and tag it.
6. **Advisory publication**: We publish the GitHub Security Advisory with CVE assignment (if applicable) and credit the reporter (unless they request anonymity).
7. **Post-release**: We monitor for regressions and update the advisory if needed.

## Scope

This policy covers the zorp Rust workspace crates published under the `aviskaar/zorp` organization on GitHub and crates.io. It does not cover:
- Dependencies (report those to their maintainers)
- Infrastructure outside this repository (e.g., crates.io, GitHub Actions)
- The desktop app distribution chain (notarization, DMG signing) — report those separately to security@aviskaar.com

## Safe Harbor

We consider good-faith research that follows this policy to be authorized. We will not pursue legal action against researchers who:
- Report through the private channel
- Avoid accessing or modifying user data
- Avoid degrading service availability
- Give us reasonable time to fix before disclosure

## Contact

For questions about this policy: security@aviskaar.com