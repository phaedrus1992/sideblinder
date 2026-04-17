# Security Policy

## Supported Versions

Only the latest release is actively supported with security fixes.

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Sideblinder is a Windows driver and userspace application with elevated
privileges. Vulnerabilities — including privilege escalation, unsafe HID
report parsing, IPC injection, or installer tampering — should be disclosed
privately.

**Contact:** anthropic@raccoonfink.com

**Response time:** You will receive an acknowledgement within 7 days. We aim
to triage and issue a fix within 90 days of the initial report, depending on
severity. We will keep you informed of progress throughout.

**Scope:** We accept reports on:
- `sideblinder-driver` (UMDF2 kernel driver)
- `sideblinder-hid` (HID parsing and FFB encoding)
- `sideblinder-app` (background service and IPC)
- `sideblinder-gui` (settings GUI)
- `sideblinder-diag` (diagnostics tool)
- Installer / uninstaller scripts

We do not accept reports for vulnerabilities in third-party dependencies
(please report those upstream). If a transitive dependency vulnerability
affects Sideblinder's security posture, we will prioritise updating it.

## Disclosure Process

1. You report privately via email.
2. We confirm receipt within 7 days.
3. We investigate, develop a fix, and coordinate a release.
4. We credit the reporter in the release notes unless they prefer to remain
   anonymous.
5. After the fix is released, you are welcome to publish a write-up.

We follow responsible disclosure and ask that you do the same.
