# Tavily Hikari Context

## System Shape

Tavily Hikari is a single-product service with one owner-facing admin surface, one user-facing console, and one active business ingress. High availability is implemented as active/standby control around that single ingress, not as a distributed cluster manager.

## HA Terms

- `full_master`: the only node allowed to handle full writes.
- `provisional_master`: a node that already owns ingress traffic but is still write-fenced for high-risk admin/business mutations.
- `standby`: a synced node that does not serve external traffic.
- `recovery`: an old master that lost ingress authority and must not take traffic back until recovery work completes.
- `planned cutover`: an operator-initiated maintenance cutover from the current `full_master` to one eligible standby candidate.
- `standby_candidate`: the only peer role hint that may receive a planned cutover in the current release.
- `observer`: a peer shown in the control plane for visibility only; it never becomes a cutover target in this release.

## Control Plane Boundaries

- The HA control plane is single-surface and active-led.
- The current `full_master` is the only node allowed to initiate `planned cutover`.
- Peer inventory is runtime-configured through `HA_PEER_NODES_JSON`; there is no UI editing path in this release.
- Timeline truth for operator-visible HA actions lives in `ha_control_plane_events` and retains only the last 7 days.

## Current Release Constraints

- Multi-node UI and model support are intentionally ahead of multi-standby orchestration support.
- At most one peer may be marked `standby_candidate`.
- `planned cutover` targets must be recently observed, synced, not stale, not recovering, and currently in `standby`.
- Emergency/manual failover remains available through local `promote` and `finalize`.
