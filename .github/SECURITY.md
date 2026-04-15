# Security Policy

This document describes how to report security vulnerabilities in the crates maintained in this
repository.

## Supported Versions

Security fixes are provided for:

- The `main` branch.
- The latest released versions of crates published from this repository.

Older versions may not receive security backports. If you are using an older release, please plan to
upgrade to a supported version to receive fixes.

## Reporting a Vulnerability

Please **do not** report security issues via public GitHub issues, pull requests, Discord, or other
public channels.

Instead, use GitHub's private vulnerability reporting:

1. Go to this repository's **Security** tab.
2. Click **Report a vulnerability** or create a **New draft security advisory**.

Include as much of the following as you can:

- Affected crate(s) and version(s), and whether you are using crates.io releases or git revisions.
- Impact and severity assessment.
- Reproduction steps and a minimal proof of concept.
- Any relevant configuration details.

If you're not sure whether something is a security issue, report it anyway and mark it as uncertain.

### Notes about AI/LLM

Fully AI-generated, low-quality, or spammy reports are not accepted. Reports must reflect a
real investigation, include repository-specific details, and provide a credible reproduction or
impact assessment. Sending AI slop or repeatedly submitting low-quality reports may result in a
ban from the organization.

## Disclosure Process

After receiving a report, maintainers will make a best effort to:

- Triage and assess impact, then work on a fix.
- Coordinate a release and publish an advisory when a fix is available.

Please keep vulnerability details confidential until an advisory is published, or maintainers confirm
it is safe to disclose.
