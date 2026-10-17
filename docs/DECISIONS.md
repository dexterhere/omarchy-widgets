# Decision log

Append newest at the bottom. Format: date, ID, decision, reason, evidence, what would change it.

## Standing decisions (from planning, 2026-10-17)

- D-0 Strategy: build on Omarchy's plugin system, do not rebuild runtime, marketplace or desktop host. Reason: they exist. Would change if the host/upstream path is closed off.
- D-0 Safety model: spec widgets plus reviewed code widgets; no sandbox claims. Would change if a separate-process host proves feasible.
- D-0 Data layer: vault plus provider daemon in v1 with a published threat model.

## Pending (decide in Phase 0)

- D-1 Authorization of admin operations on the daemon socket.
- D-2 Host strategy (contribute / fork / standalone).
- D-3 Project name and fallback.
- D-4 Sidecar `widget.json` vs folded into manifest.
- D-5 Shared bridge singleton vs per-widget client.
- D-6 Monorepo + mirror repos vs single-commit fetch for monorepo widgets.
- D-7 Default Center shortcut chord.
