# Security policy

This project handles credentials and personal data (mail, calendar, tokens). Security reports get priority over feature work.

## Reporting a vulnerability

Do not open a public issue. Use GitHub private vulnerability reporting:
<https://github.com/dexterhere/omarchy-widgets/security/advisories/new>

Include what you found, how to reproduce it, and the impact. We aim to acknowledge within 72 hours. Please allow time for a fix before public disclosure.

## Scope and honest limits

- In scope: the daemon (`owd`), the vault, the wire protocol, grants, the `ow` CLI, and release artifacts.
- Code widgets (QML) are not sandboxed. They run as the user and can read anything the user can. This is a documented limit, not a vulnerability by itself.
- Spec widgets run no code and see only granted fields. A way around that is in scope.

## Rules the code must keep

Secrets never appear in QML, argv, environment, logs, config files, crash dumps or previews. See `CLAUDE.md` "Hard rules".
