# Security Policy

## Reporting a vulnerability

Report security issues privately through GitHub's private vulnerability
reporting: open the [Security tab](https://github.com/aviskaar/zorp/security)
and press **Report a vulnerability**, or go straight to
[the advisory form](https://github.com/aviskaar/zorp/security/advisories/new).
This is the only reporting channel. Do not open a public issue, pull request
or discussion about a vulnerability.

Include:

- The affected component or crate, for example `zorp-web` or `zorp-agent`.
- The version (`zorp-agent --version`) and the surface: terminal, browser or
  Mac app.
- Steps to reproduce or a proof of concept.
- The impact you see.
- A suggested fix or mitigation, if you have one.

`zorp-agent doctor` prints no secret by design, so its output is safe to
attach. Never attach an API key.

## Supported versions

Only the latest release receives security fixes. There are no long-term
support branches, so update to the latest release to get a fix.

| Version | Supported |
|---------|-----------|
| Latest release | Yes |
| Older releases | No |

## Response expectations

- **Acknowledgment** within 3 business days.
- **Initial assessment** of severity and scope within 10 business days.
- **Status updates** at least every 14 days until the issue is resolved.
- **Fix target** of 30 days from acknowledgment for critical and high
  severity issues. Medium and low severity issues may take longer.

## Disclosure process

1. You report privately through the advisory form.
2. We confirm the issue and assess its severity in the advisory.
3. We develop and test a fix in the advisory's temporary private fork.
4. We agree a disclosure date with you. The default is 90 days from
   acknowledgment, or sooner once a fix has shipped.
5. We ship the fix in a new tagged release.
6. We publish the advisory, request a CVE where one applies, and credit you
   unless you ask not to be named.

## Scope

This policy covers the code in this repository: the Rust workspace crates,
the browser UI in `web/`, the Mac desktop app in `zorp-desktop/`, and the
release artifacts built from it. It does not cover:

- Third-party dependencies. Report those to their maintainers.
- Infrastructure outside this repository, such as GitHub or a model
  provider's API.

## Safe harbor

We consider good-faith research that follows this policy to be authorized,
and we will not pursue legal action against researchers who:

- Report through the private channel.
- Avoid accessing or modifying other people's data.
- Avoid degrading service availability.
- Give us reasonable time to fix the issue before disclosing it.
