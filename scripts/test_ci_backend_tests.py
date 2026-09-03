#!/usr/bin/env python3

import argparse
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ci_backend_tests.py")
SPEC = importlib.util.spec_from_file_location("ci_backend_tests", SCRIPT_PATH)
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class BackendTestRunnerContractTests(unittest.TestCase):
    def test_v2_bundle_deduplicates_and_verifies_checksum(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            executable = root / "executables" / "fixture-abc123"
            executable.parent.mkdir()
            executable.write_bytes(b"fixture executable")
            digest = RUNNER.sha256_file(executable)
            support_binary = root / "executables" / "support-source"
            support_binary.write_bytes(b"fixture support binary")
            support_digest = RUNNER.sha256_file(support_binary)
            manifest = {
                "format_version": RUNNER.ARTIFACT_FORMAT_VERSION,
                "executables": {
                    digest: {
                        "path": "executables/fixture-abc123",
                        "sha256": digest,
                        "name": "fixture",
                        "tests": ["fixture::test"],
                    },
                    support_digest: {
                        "path": "executables/support-source",
                        "sha256": support_digest,
                        "name": "observability_lock_holder",
                    },
                },
                "coverage_targets": {
                    "lib": {
                        "executables": [digest],
                        "support_binaries": {"FIXTURE_BIN": support_digest},
                    }
                },
            }
            (root / RUNNER.ARTIFACT_MANIFEST_NAME).write_text(
                json.dumps(manifest), encoding="utf-8"
            )

            executables, support_binaries = RUNNER.load_prebuilt_executables(root, "lib")

            self.assertEqual(executables[0]["tests"], ["fixture::test"])
            self.assertEqual(
                Path(support_binaries["FIXTURE_BIN"]), support_binary.resolve()
            )
            self.assertEqual(
                RUNNER.artifact_executable_path(
                    root, support_digest, "observability_lock_holder"
                ).name,
                f"observability_lock_holder-{support_digest}",
            )
            executable.write_bytes(b"tampered")
            with self.assertRaises(SystemExit):
                RUNNER.load_prebuilt_executables(root, "lib")

    def test_v1_bundle_remains_readable(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            target_dir = Path(temp_dir) / RUNNER.artifact_target_dir_name("lib")
            target_dir.mkdir()
            executable = target_dir / "legacy"
            executable.write_bytes(b"legacy executable")
            (target_dir / "tests.json").write_text(
                json.dumps({"legacy": ["legacy::test"]}), encoding="utf-8"
            )
            (target_dir / "support_binaries.json").write_text("{}", encoding="utf-8")

            executables, support_binaries = RUNNER.load_prebuilt_executables(temp_dir, "lib")

            self.assertEqual(executables[0]["name"], "legacy")
            self.assertEqual(executables[0]["tests"], ["legacy::test"])
            self.assertEqual(support_binaries, {})

    def test_bundle_stages_a_portable_lane_runner(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            RUNNER.stage_lane_runner(root)

            runtime_runner = root / "scripts" / "ci_backend_tests.py"
            runtime_manifest = root / "scripts" / "ci_backend_test_manifest.json"
            self.assertTrue(runtime_runner.is_file())
            self.assertTrue(runtime_manifest.is_file())
            self.assertTrue((root / "source" / "src" / "lib.rs").is_file())
            self.assertTrue((root / "source" / "tests" / "rust_source_line_budgets.rs").is_file())

            completed = subprocess.run(
                [sys.executable, str(runtime_runner), "lane-matrix", "--lane-count", "16"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            )
            lanes = json.loads(completed.stdout)
            self.assertEqual(lanes[0]["id"], "lane-01")

    def test_lane_matrix_is_stable_lpt(self):
        shards = [
            {"id": "gamma", "estimated_seconds": 5},
            {"id": "alpha", "estimated_seconds": 8},
            {"id": "beta", "estimated_seconds": 8},
            {"id": "delta", "estimated_seconds": 3},
        ]

        lanes = RUNNER.build_lane_matrix(shards, 2)

        self.assertEqual(
            lanes,
            [
                {
                    "id": "lane-01",
                    "name": "Lane 01",
                    "estimated_seconds": 13,
                    "shard_ids": ["alpha", "gamma"],
                },
                {
                    "id": "lane-02",
                    "name": "Lane 02",
                    "estimated_seconds": 11,
                    "shard_ids": ["beta", "delta"],
                },
            ],
        )

    def test_reconciliation_shards_are_mutually_exclusive(self):
        _, shards = RUNNER.load_manifest()
        reconciliation_shards = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "lib-reconciliation-maintenance",
                "lib-upstream-reconciliation",
                "lib-reconciliation-projection",
            }
        }

        self.assertEqual(
            set(reconciliation_shards),
            {
                "lib-reconciliation-maintenance",
                "lib-upstream-reconciliation",
                "lib-reconciliation-projection",
            },
        )
        upstream = reconciliation_shards["lib-upstream-reconciliation"]
        self.assertEqual(RUNNER.shard_resource_limits(upstream, 4, 2), (4, 1))
        self.assertEqual(len(upstream["include_prefixes"]), 29)
        self.assertEqual(len(upstream["include_prefixes"]), len(set(upstream["include_prefixes"])))
        self.assertTrue(
            all(
                prefix.startswith("tests::upstream_reconciliation::")
                for prefix in upstream["include_prefixes"]
            )
        )
        self.assertIn(
            "tests::upstream_reconciliation::reconciliation_waits_for_a_complete_eligible_period",
            upstream["include_prefixes"],
        )
        prefixes = [
            prefix
            for shard in reconciliation_shards.values()
            for prefix in shard["include_prefixes"]
        ]
        self.assertEqual(len(prefixes), len(set(prefixes)))
        self.assertEqual(
            set(prefixes),
            {
                "remote_attempt_admission::tests::",
                "tests::maintenance_queue_performance::",
                "tests::schema_migrations::",
                "tests::reconciliation_controller::",
                "tests::upstream_reconciliation_continuation::",
                "tests::upstream_reconciliation_engine::",
                "tests::upstream_reconciliation_fencing::",
                "tests::upstream_reconciliation_projection::",
                "tavily_proxy::reconciliation_engine_tests::",
                "tavily_proxy::user_business_calls_memory::memory_window_regression_tests::",
                "upstream_privacy::tests::",
            }.union(upstream["include_prefixes"]),
        )

    def test_request_rollup_storage_shards_are_mutually_exclusive(self):
        _, shards = RUNNER.load_manifest()
        storage_shards = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "lib-request-rollup-storage",
                "lib-request-log-retention-maintenance",
                "lib-request-log-retention-policy",
                "lib-scheduled-request-maintenance",
            }
        }

        self.assertEqual(
            set(storage_shards),
            {
                "lib-request-rollup-storage",
                "lib-request-log-retention-maintenance",
                "lib-request-log-retention-policy",
                "lib-scheduled-request-maintenance",
            },
        )
        prefixes = [
            prefix for shard in storage_shards.values() for prefix in shard["include_prefixes"]
        ]
        self.assertEqual(len(prefixes), len(set(prefixes)))
        self.assertEqual(
            set(prefixes),
            {
                "tests::jobs_and_request_log_retention::request_",
                "tests::observability_and_lifecycle::request_",
                "tests::request_kind_and_core::request_",
                "tests::request_rollup::request_",
                "tests::usage_series_and_backfills::request_",
                "tests::user_tokens_and_pending_billing::request_",
                "tests::jobs_and_request_log_retention::startup_",
                "tests::request_kind_and_core::startup_",
                "tests::request_rollup::startup_",
                "tests::jobs_and_request_log_retention::quota_",
                "tests::request_rollup::quota_",
                "tests::jobs_and_request_log_retention::standalone_request_logs_gc_",
                "tests::ha_outbox_and_compaction::standalone_ha_outbox_gc_",
                "tests::request_kind_and_core::successful_",
                "tests::jobs_and_request_log_retention::scheduled_",
                "tests::request_rollup::suppressed_",
            },
        )

    def test_request_log_retention_shards_preserve_serial_policy_execution(self):
        _targets, shards = RUNNER.load_manifest()
        retention_shards = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "lib-request-log-retention-maintenance",
                "lib-request-log-retention-policy",
            }
        }

        self.assertEqual(
            set(retention_shards),
            {
                "lib-request-log-retention-maintenance",
                "lib-request-log-retention-policy",
            },
        )
        self.assertEqual(
            retention_shards["lib-request-log-retention-maintenance"]["include_prefixes"],
            ["tests::jobs_and_request_log_retention::standalone_request_logs_gc_"],
        )
        self.assertEqual(
            retention_shards["lib-request-log-retention-policy"]["include_prefixes"],
            ["tests::jobs_and_request_log_retention::request_"],
        )
        self.assertEqual(
            retention_shards["lib-request-log-retention-policy"]["serial_prefixes"],
            ["tests::jobs_and_request_log_retention::request_"],
        )

    def test_admin_api_resource_shards_are_mutually_exclusive(self):
        _, shards = RUNNER.load_manifest()
        resource_shards = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "bin-admin-api-identity",
                "bin-admin-api-sse",
                "bin-admin-api-observability",
                "bin-admin-api-settings",
            }
        }

        self.assertEqual(
            set(resource_shards),
            {
                "bin-admin-api-identity",
                "bin-admin-api-sse",
                "bin-admin-api-observability",
                "bin-admin-api-settings",
            },
        )
        prefixes = [
            prefix for shard in resource_shards.values() for prefix in shard["include_prefixes"]
        ]
        self.assertEqual(len(prefixes), len(set(prefixes)))
        self.assertEqual(
            set(prefixes),
            {
                "server::admin_alerts_prewarm_tests::",
                "server::tests::admin_logs_and_summary::admin_",
                "server::tests::admin_token_filters_and_maintenance::admin_",
                "server::tests::admin_analysis_pressure::analysis_",
                "server::tests::admin_token_owner_summary::admin_",
                "server::tests::admin_users_and_tokens::account_",
                "server::tests::admin_users_and_tokens::admin_",
                "server::tests::admin_users_and_tokens::admin_dashboard_sse_snapshot_refreshes_when_recent_alerts_change",
                "server::tests::admin_users_shadow_daily_projection::list_",
                "server::tests::dashboard_overview_snapshot::admin_",
                "server::tests::log_catalog_and_dashboard_sse::admin_",
                "server::tests::system_settings_and_forward_proxy::admin_",
                "server::tests::system_settings_reconciliation_status::admin_",
                "server::tests::tavily_http_search::admin_",
                "server::tests::admin_logs_and_summary::api_",
                "server::tests::api_keys_and_registration::api_",
                "server::tests::branded_assets_contract::",
                "server::pwa_cache_tests::",
                "server::tests::linuxdo_oauth_and_admin_keys::api_",
                "server::tests::log_catalog_and_dashboard_sse::api_",
                "server::admin_resources_tests::",
                "server::dto_tests::",
            },
        )

    def test_ci_lane_count_stays_within_the_maximum(self):
        lanes = RUNNER.build_lane_matrix(
            [{"id": f"shard-{index}", "estimated_seconds": index + 1} for index in range(25)],
            12,
        )

        self.assertEqual(len(lanes), 12)
        self.assertLessEqual(len(lanes), 16)

    def test_low_resource_environment_is_explicit(self):
        environment = RUNNER.cargo_environment(cargo_jobs=2, web_assets_dir="/tmp/assets")

        self.assertEqual(environment["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(environment[RUNNER.WEB_ASSET_ENV], "/tmp/assets")
        self.assertEqual(RUNNER.DEFAULT_LOW_RESOURCE_FILTERED_PROCESS_WORKERS, 1)
        self.assertEqual(RUNNER.DEFAULT_LOW_RESOURCE_FILTERED_TEST_THREADS, 2)

    def test_diagnostic_resources_override_low_resource_defaults(self):
        resources = RUNNER.resources_from_args(
            argparse.Namespace(
                diagnostic=True,
                cargo_jobs=RUNNER.DEFAULT_LOW_RESOURCE_CARGO_JOBS,
                filtered_process_workers=RUNNER.DEFAULT_LOW_RESOURCE_FILTERED_PROCESS_WORKERS,
                filtered_test_threads=RUNNER.DEFAULT_LOW_RESOURCE_FILTERED_TEST_THREADS,
            )
        )

        self.assertEqual(resources, (1, 1, 1))

    def test_requested_workers_do_not_exceed_shard_limit(self):
        workers, threads = RUNNER.shard_resource_limits(
            {"filtered_process_workers": 2, "filtered_test_threads": 1},
            filtered_process_workers=3,
            filtered_test_threads=2,
        )

        self.assertEqual((workers, threads), (2, 1))

    def test_minimal_web_assets_meet_the_web_asset_contract(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            RUNNER.write_minimal_web_assets(temp_dir)

            RUNNER.verify_web_assets(temp_dir)

    def test_minimal_web_assets_do_not_rewrite_unchanged_files(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fixture_dir = Path(temp_dir)
            RUNNER.write_minimal_web_assets(fixture_dir)
            asset_path = fixture_dir / "index.html"
            initial_mtime = asset_path.stat().st_mtime_ns

            RUNNER.write_minimal_web_assets(fixture_dir)

            self.assertEqual(asset_path.stat().st_mtime_ns, initial_mtime)

    def test_ci_test_fixture_path_is_stable(self):
        fixture_dir = RUNNER.minimal_web_assets_dir("ci-test")

        self.assertEqual(fixture_dir, RUNNER.ROOT / "target" / "ci-test" / "ci-web-assets")

    def test_backend_bundle_cache_skips_cold_setup_on_an_exact_input_hit(self):
        workflow = (RUNNER.ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        bundle_cache_block = workflow.split("- name: Cache prepared backend test bundle", 1)[1].split(
            "- name: Install system dependencies", 1
        )[0]

        for cache_input in (
            "Cargo.lock",
            "Cargo.toml",
            "build.rs",
            "rust-toolchain.toml",
            ".cargo/**",
            "src/**",
            "tests/**",
            "scripts/ci_backend_tests.py",
            "scripts/ci_backend_test_manifest.json",
        ):
            self.assertIn(cache_input, bundle_cache_block)
        self.assertIn("id: backend-test-bundle-cache", bundle_cache_block)
        self.assertIn("backend-test-bundle-v1-rust-1.91.0-ci-test-mold-", bundle_cache_block)
        self.assertNotIn("Refresh exact compilation cache timestamps", workflow)
        cold_setup_guard = "steps.backend-test-bundle-cache.outputs.cache-hit != 'true'"
        self.assertEqual(workflow.count(cold_setup_guard), 5)

    def test_request_rollup_integrity_has_one_manifest_owner(self):
        _targets, shards = RUNNER.load_manifest()
        storage = next(shard for shard in shards if shard["id"] == "lib-request-rollup-storage")
        integrity = next(
            shard for shard in shards if shard["id"] == "lib-request-rollup-integrity"
        )

        self.assertNotIn("tests::dashboard_rollup_integrity::", storage["include_prefixes"])
        self.assertEqual(
            integrity["include_prefixes"], ["tests::dashboard_rollup_integrity::"]
        )

    def test_linuxdo_dashboard_overview_has_one_manifest_owner(self):
        _targets, shards = RUNNER.load_manifest()
        forward = next(shard for shard in shards if shard["id"] == "bin-linuxdo-forward")
        dashboard = next(
            shard for shard in shards if shard["id"] == "bin-linuxdo-dashboard-overview"
        )
        dashboard_prefix = "server::tests::dashboard_overview_snapshot::dashboard_"

        self.assertIn(dashboard_prefix, forward["exclude_prefixes"])
        self.assertEqual(dashboard["include_prefixes"], [dashboard_prefix])
        self.assertEqual(dashboard["filtered_test_threads"], 2)
        self.assertEqual(forward["estimated_seconds"], 41)
        self.assertEqual(dashboard["estimated_seconds"], 51)

    def test_tavily_http_search_runs_as_isolated_exact_tests(self):
        _targets, shards = RUNNER.load_manifest()
        tavily_http = next(shard for shard in shards if shard["id"] == "bin-tavily-http")
        search_prefix = "server::tests::tavily_http_search::tavily_"

        self.assertEqual(tavily_http["estimated_seconds"], 68)
        self.assertIn(search_prefix, tavily_http["include_prefixes"])
        self.assertEqual(tavily_http["isolated_prefixes"], [search_prefix])
        self.assertEqual(tavily_http["isolated_process_workers"], 4)
        self.assertEqual(RUNNER.shard_resource_limits(tavily_http, 4, 2), (4, 1))
        self.assertEqual(RUNNER.isolated_process_worker_limit(tavily_http, 4), 4)
        self.assertEqual(RUNNER.isolated_process_worker_limit(tavily_http, 2), 2)

        reporting = next(
            shard for shard in shards if shard["id"] == "lib-request-rollup-reporting"
        )
        self.assertEqual(RUNNER.isolated_process_worker_limit(reporting, 4), 1)

    def test_mcp_billing_runs_as_isolated_exact_tests(self):
        _targets, shards = RUNNER.load_manifest()
        billing = next(shard for shard in shards if shard["id"] == "bin-mcp-billing")
        billing_prefix = "server::tests::mcp_billing_and_sessions::mcp_"

        self.assertEqual(billing["include_prefixes"], [billing_prefix])
        self.assertEqual(billing["isolated_prefixes"], [billing_prefix])
        self.assertEqual(billing["isolated_process_workers"], 4)
        self.assertEqual(RUNNER.isolated_process_worker_limit(billing, 4), 4)
        self.assertEqual(RUNNER.isolated_process_worker_limit(billing, 2), 2)

    def test_ha_lifecycle_shards_separate_event_and_recovery_tests(self):
        _targets, shards = RUNNER.load_manifest()
        selected = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "bin-ha-rest-lifecycle-state-base",
                "bin-ha-rest-lifecycle-ha-events",
                "bin-ha-rest-lifecycle-ha-recovery",
            }
        }

        self.assertEqual(
            set(selected),
            {
                "bin-ha-rest-lifecycle-state-base",
                "bin-ha-rest-lifecycle-ha-events",
                "bin-ha-rest-lifecycle-ha-recovery",
            },
        )
        parent_prefix = "server::tests::alerts_and_ha::ha_"
        self.assertNotIn(
            parent_prefix,
            selected["bin-ha-rest-lifecycle-state-base"]["include_prefixes"],
        )
        self.assertIn(
            "server::tests::alerts_and_ha::ha_events_",
            selected["bin-ha-rest-lifecycle-ha-events"]["include_prefixes"],
        )
        self.assertIn(
            "server::tests::alerts_and_ha::ha_sync_",
            selected["bin-ha-rest-lifecycle-ha-recovery"]["include_prefixes"],
        )

    def test_sensitive_shards_cap_ci_parallelism(self):
        _targets, shards = RUNNER.load_manifest()
        reporting = next(
            shard for shard in shards if shard["id"] == "lib-request-rollup-reporting"
        )
        reporting_slice = next(
            shard for shard in shards if shard["id"] == "lib-request-rollup-reporting-slice"
        )
        reporting_admitted_flush = next(
            shard
            for shard in shards
            if shard["id"] == "lib-request-rollup-reporting-admitted-flush"
        )
        scheduled_maintenance = next(
            shard for shard in shards if shard["id"] == "lib-scheduled-request-maintenance"
        )
        scheduled_maintenance_claim = next(
            shard
            for shard in shards
            if shard["id"] == "lib-scheduled-request-maintenance-claim"
        )
        alert = next(shard for shard in shards if shard["id"] == "lib-alert-projection")
        affinity = next(shard for shard in shards if shard["id"] == "lib-affinity-domain")
        admin_identity = next(
            shard for shard in shards if shard["id"] == "bin-admin-api-identity"
        )
        admin_observability = next(
            shard for shard in shards if shard["id"] == "bin-admin-api-observability"
        )
        admin_settings = next(
            shard for shard in shards if shard["id"] == "bin-admin-api-settings"
        )
        admin_sse = next(shard for shard in shards if shard["id"] == "bin-admin-api-sse")
        admin_lifecycle = next(
            shard for shard in shards if shard["id"] == "bin-admin-api-lifecycle"
        )
        operational = next(
            shard for shard in shards if shard["id"] == "lib-operational-maintenance"
        )
        user_business_calls = next(
            shard for shard in shards if shard["id"] == "lib-user-business-calls"
        )
        mcp_research_protocol = next(
            shard for shard in shards if shard["id"] == "bin-mcp-research-protocol"
        )
        mcp_research_batch = next(
            shard for shard in shards if shard["id"] == "bin-mcp-research-batch"
        )
        mcp_system = next(shard for shard in shards if shard["id"] == "bin-mcp-system")
        mcp_rebalance_session = next(
            shard for shard in shards if shard["id"] == "bin-mcp-rebalance-session"
        )
        mcp_rebalance_control = next(
            shard for shard in shards if shard["id"] == "bin-mcp-rebalance-control"
        )

        self.assertEqual(
            reporting["isolated_prefixes"],
            ["tests::request_rollup_public_metrics::admin_"],
        )
        reporting_slice_test = (
            "tests::request_rollup_public_metrics::request_stats_background_slice_bounds_work_and_requeues_the_tail"
        )
        self.assertIn(reporting_slice_test, reporting["exclude_prefixes"])
        self.assertEqual(reporting_slice["include_prefixes"], [reporting_slice_test])
        self.assertEqual(reporting_slice["serial_prefixes"], [reporting_slice_test])
        self.assertEqual(RUNNER.shard_resource_limits(reporting_slice, 4, 2), (1, 1))
        admitted_flush_test = (
            "tests::request_rollup_public_metrics::admitted_flush_drains_followup_batches_before_reporting_fresh"
        )
        self.assertIn(admitted_flush_test, reporting["exclude_prefixes"])
        self.assertEqual(reporting_admitted_flush["include_prefixes"], [admitted_flush_test])
        self.assertEqual(reporting_admitted_flush["serial_prefixes"], [admitted_flush_test])
        self.assertEqual(RUNNER.shard_resource_limits(reporting_admitted_flush, 4, 2), (1, 1))
        scheduled_claim_test = (
            "tests::jobs_and_request_log_retention::scheduled_job_claim_serializes_concurrent_duplicate_triggers"
        )
        self.assertIn(scheduled_claim_test, scheduled_maintenance["exclude_prefixes"])
        self.assertEqual(scheduled_maintenance_claim["include_prefixes"], [scheduled_claim_test])
        self.assertEqual(scheduled_maintenance_claim["serial_prefixes"], [scheduled_claim_test])
        self.assertEqual(RUNNER.shard_resource_limits(scheduled_maintenance_claim, 4, 2), (1, 1))
        self.assertEqual(RUNNER.shard_resource_limits(alert, 3, 2), (3, 1))
        self.assertEqual(RUNNER.shard_resource_limits(affinity, 3, 2), (3, 2))
        self.assertEqual(RUNNER.shard_resource_limits(admin_identity, 3, 2), (2, 2))
        self.assertEqual(RUNNER.shard_resource_limits(admin_identity, 4, 2), (2, 2))
        self.assertEqual(RUNNER.shard_resource_limits(admin_observability, 3, 2), (2, 2))
        self.assertEqual(RUNNER.shard_resource_limits(admin_settings, 3, 2), (2, 2))
        sse_test = "server::tests::admin_users_and_tokens::admin_dashboard_sse_snapshot_refreshes_when_recent_alerts_change"
        self.assertIn(sse_test, admin_identity["exclude_prefixes"])
        self.assertEqual(admin_sse["include_prefixes"], [sse_test])
        self.assertEqual(RUNNER.shard_resource_limits(admin_sse, 4, 2), (1, 1))
        self.assertEqual(RUNNER.shard_resource_limits(operational, 4, 2), (4, 1))
        self.assertIn(
            "server::tests::alerts_and_ha_dashboard_defaults::admin_alerts_pressure_uses_same_key_last_good_and_reports_cold_misses",
            admin_lifecycle["serial_prefixes"],
        )
        self.assertIn(
            "tests::user_business_calls_1h::user_",
            user_business_calls["serial_prefixes"],
        )
        self.assertIn(
            "server::tests::system_settings_and_forward_proxy::mcp_",
            mcp_system["serial_prefixes"],
        )
        self.assertIn(
            "server::tests::mcp_rebalance_and_follow_up::mcp_session_affinity_",
            mcp_rebalance_session["include_prefixes"],
        )
        self.assertIn(
            "server::tests::mcp_rebalance_and_follow_up::mcp_rebalance_",
            mcp_rebalance_control["include_prefixes"],
        )
        mcp_prefix = "server::tests::research_result_and_mcp_subpath::mcp_"
        batch_prefix = "server::tests::research_result_and_mcp_subpath::mcp_batch_"
        self.assertEqual(mcp_research_protocol["include_prefixes"], [mcp_prefix])
        self.assertEqual(mcp_research_protocol["exclude_prefixes"], [batch_prefix])
        self.assertEqual(mcp_research_protocol["serial_prefixes"], [mcp_prefix])
        self.assertEqual(mcp_research_batch["include_prefixes"], [batch_prefix])
        self.assertEqual(mcp_research_batch["serial_prefixes"], [batch_prefix])

    def test_account_identity_shards_preserve_semantic_boundaries(self):
        _targets, shards = RUNNER.load_manifest()
        selected = {
            shard["id"]: shard
            for shard in shards
            if shard["id"]
            in {
                "lib-account-linuxdo-identity",
                "lib-user-token-identity",
                "lib-user-business-calls",
                "lib-account-legacy-identity",
            }
        }

        self.assertEqual(
            set(selected),
            {
                "lib-account-linuxdo-identity",
                "lib-user-token-identity",
                "lib-user-business-calls",
                "lib-account-legacy-identity",
            },
        )
        self.assertIn(
            "tests::account_quota_and_billing::account_",
            selected["lib-account-linuxdo-identity"]["include_prefixes"],
        )
        self.assertIn(
            "tests::user_tokens_and_pending_billing::token_",
            selected["lib-user-token-identity"]["include_prefixes"],
        )
        self.assertEqual(
            selected["lib-user-business-calls"]["serial_prefixes"],
            ["tests::user_business_calls_1h::user_"],
        )
        self.assertIn(
            "tests::request_rollup::legacy_",
            selected["lib-account-legacy-identity"]["include_prefixes"],
        )

    def test_serial_parent_filter_safely_skips_a_child_prefix(self):
        parent_prefix = "server::tests::research::mcp_"
        batch_prefix = "server::tests::research::mcp_batch_"
        protocol_test = "server::tests::research::mcp_initialize"
        batch_test = "server::tests::research::mcp_batch_tools_call"
        shard = {
            "include_prefixes": [parent_prefix],
            "exclude_prefixes": [batch_prefix],
            "serial_prefixes": [parent_prefix],
            "isolated_prefixes": [],
        }

        filters, serial_filters, exact_fallback, isolated_tests = RUNNER.select_safe_filter_groups(
            [protocol_test, batch_test], shard
        )

        self.assertEqual(filters, [])
        self.assertEqual(serial_filters, [(parent_prefix, (batch_prefix,))])
        self.assertEqual(exact_fallback, [])
        self.assertEqual(isolated_tests, [])
        with mock.patch.object(RUNNER, "run_parallel_test_commands") as run_commands:
            RUNNER.run_filtered_tests_with_env(
                "/tmp/test-binary", serial_filters, 1, 1
            )
        run_commands.assert_called_once_with(
            [
                [
                    "/tmp/test-binary",
                    "--test-threads=1",
                    "--skip",
                    batch_prefix,
                    parent_prefix,
                ]
            ],
            max_workers=1,
            extra_env=None,
        )

    def test_isolated_prefixes_run_as_serial_exact_tests(self):
        executable_tests = [
            "tests::safe::one",
            "tests::request_rollup_public_metrics::admin_one",
            "tests::request_rollup_public_metrics::admin_two",
        ]
        shard = {
            "include_prefixes": [
                "tests::safe::",
                "tests::request_rollup_public_metrics::admin_",
            ],
            "exclude_prefixes": [],
            "serial_prefixes": [],
            "isolated_prefixes": ["tests::request_rollup_public_metrics::admin_"],
        }

        filters, serial_filters, exact_fallback, isolated_tests = RUNNER.select_safe_filter_groups(
            executable_tests, shard
        )

        self.assertEqual(filters, ["tests::safe::"])
        self.assertEqual(serial_filters, [])
        self.assertEqual(exact_fallback, [])
        self.assertEqual(
            isolated_tests,
            [
                "tests::request_rollup_public_metrics::admin_one",
                "tests::request_rollup_public_metrics::admin_two",
            ],
        )

    def test_nonserial_exclusion_keeps_a_child_test_out_of_parent_filter(self):
        special_test = "server::tests::admin::dashboard_sse"
        shard = {
            "include_prefixes": ["server::tests::admin::"],
            "exclude_prefixes": [special_test],
            "serial_prefixes": [],
            "isolated_prefixes": [],
        }

        filters, serial_filters, exact_fallback, isolated_tests = RUNNER.select_safe_filter_groups(
            [special_test, "server::tests::admin::other"], shard
        )

        self.assertEqual(filters, [])
        self.assertEqual(serial_filters, [])
        self.assertEqual(exact_fallback, ["server::tests::admin::other"])
        self.assertEqual(isolated_tests, [])

    def test_manifest_lpt_stays_within_the_lane_budget(self):
        _targets, shards = RUNNER.load_manifest()
        lanes = RUNNER.build_lane_matrix(shards, 16)

        self.assertEqual(len(lanes), 16)
        self.assertLessEqual(max(lane["estimated_seconds"] for lane in lanes), 120)


if __name__ == "__main__":
    unittest.main()
