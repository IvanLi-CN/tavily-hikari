# ADR 0001: HA Planned Cutover Uses a Single Active-Led Control Surface

## Status

Accepted

## Context

The existing HA admin UI exposed only local status plus inferred remote placeholders. That was insufficient for routine maintenance operations such as deliberately draining the active node before shutdown. Operators needed a real control plane with explicit peer visibility, an auditable planned cutover path, and a bounded event history.

## Decision

- `planned cutover` is the formal term for maintenance cutover.
- The current `full_master` admin surface is the only place where planned cutover is initiated.
- Peer membership is configured through `HA_PEER_NODES_JSON`.
- Only one peer may be tagged `standby_candidate` in the current release.
- Peer topology is visible in the UI, but only the `standby_candidate` can become a planned cutover target.
- HA operator-visible timeline data is normalized into `ha_control_plane_events` and retained for 7 days only.

## Consequences

- Operators gain a single auditable path for maintenance cutover without logging into the target node to finish the switch.
- The system stays intentionally conservative: multi-peer visibility exists now, but multi-standby takeover semantics do not.
- Timeline storage is operationally bounded and query-friendly.
- Peer membership changes remain a deployment/configuration concern instead of an in-app cluster-management feature.
