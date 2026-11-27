-- Singleton rows the reconciler and HA readers assume exist.
--
-- The SQLite schema seeds these with `INSERT ... ON CONFLICT DO NOTHING` from the migration
-- code, so every reader can select `WHERE id = 'local'`. The structural baseline only created
-- the tables; without these rows a PostgreSQL read returns zero rows and the caller treats the
-- process as unconfigured.
--
-- Every NOT NULL column in these tables carries a default, so the seed only names the key.

INSERT INTO core.upstream_reconciliation_projection_state (id) VALUES ('local')
    ON CONFLICT (id) DO NOTHING;

INSERT INTO core.upstream_reconciliation_run_observation (id) VALUES ('local')
    ON CONFLICT (id) DO NOTHING;

INSERT INTO core.upstream_reconciliation_control_state (id) VALUES ('local')
    ON CONFLICT (id) DO NOTHING;

INSERT INTO core.upstream_reconciliation_research_progress_window (id) VALUES ('local')
    ON CONFLICT (id) DO NOTHING;

INSERT INTO core.upstream_reconciliation_research_scan_state (id) VALUES ('local')
    ON CONFLICT (id) DO NOTHING;

-- `core.ha_node_state` is deliberately not seeded: its node identity columns have no defaults
-- and HA retires at cutover (ADR 0004), so fabricating a node row would invent state.

-- Per-source projection state. Every NOT NULL column carries a default, so the seed names only
-- the source. The three sources mirror `ALERT_PROJECTION_SOURCES`.
INSERT INTO observability.dashboard_alert_projection_state (source_kind) VALUES ('auth_token_log')
    ON CONFLICT (source_kind) DO NOTHING;
INSERT INTO observability.dashboard_alert_projection_state (source_kind) VALUES ('api_key_maintenance_record')
    ON CONFLICT (source_kind) DO NOTHING;
INSERT INTO observability.dashboard_alert_projection_state (source_kind) VALUES ('scheduled_job')
    ON CONFLICT (source_kind) DO NOTHING;

INSERT INTO observability.dashboard_alert_projection_history_state (source_kind) VALUES ('auth_token_log')
    ON CONFLICT (source_kind) DO NOTHING;
INSERT INTO observability.dashboard_alert_projection_history_state (source_kind) VALUES ('api_key_maintenance_record')
    ON CONFLICT (source_kind) DO NOTHING;
INSERT INTO observability.dashboard_alert_projection_history_state (source_kind) VALUES ('scheduled_job')
    ON CONFLICT (source_kind) DO NOTHING;

INSERT INTO observability.dashboard_alert_projection_revision_state (singleton) VALUES (1)
    ON CONFLICT (singleton) DO NOTHING;

INSERT INTO observability.admin_alert_canonical_groups_state (singleton) VALUES (1)
    ON CONFLICT (singleton) DO NOTHING;
