# Rebalance audit rollup recovery

Dashboard rollup recovery identifies retained rebalance audit records by their durable gateway, experiment, and upstream-operation markers, not by request classification fields that canonicalization may fill during insertion. Advancing the recovery semantic version reopens a previously completed operation so the existing fenced, bounded, zero-maintenance worker can replace affected minute buckets and re-audit their sealed days without modifying the source logs or request semantics.
