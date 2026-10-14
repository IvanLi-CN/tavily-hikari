#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
from pathlib import Path
from typing import Any

import yaml
from yaml.constructor import ConstructorError
from yaml.loader import SafeLoader


EXPECTED_QUERY_FRAGMENTS: dict[str, tuple[str, ...]] = {
    "hikari-exporter-unavailable": ('up{job="tavily-hikari"} == bool 0',),
    "hikari-scrape-stale": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 1",
    ),
    "hikari-error-rate-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:error_rate5m > 0.2",
        "hikari:request_rate5m > 0.1",
    ),
    "hikari-request-p95-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:request_p95_seconds5m > 2",
    ),
    "hikari-sqlite-wait-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:sqlite_wait_p95_seconds5m > 0.5",
    ),
    "hikari-memory-headroom-low": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 1",
        "hikari:memory_headroom_bytes < 134217728",
    ),
}


class UniqueKeyLoader(SafeLoader):
    pass


def construct_unique_mapping(
    loader: UniqueKeyLoader, node: yaml.MappingNode, deep: bool = False
) -> dict[Any, Any]:
    mapping: dict[Any, Any] = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise ConstructorError(
                "while constructing a mapping",
                node.start_mark,
                f"found duplicate key {key!r}",
                key_node.start_mark,
            )
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,
    construct_unique_mapping,
)


def fail(message: str) -> None:
    raise SystemExit(message)


def mapping(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{context} must be a mapping")
    return value


def exact(value: Any, expected: Any, context: str) -> None:
    if value != expected:
        fail(f"{context}: expected {expected!r}, found {value!r}")


def validate_rule(rule: Any) -> tuple[str, str]:
    rule_map = mapping(rule, "alert rule")
    uid = rule_map.get("uid")
    if uid not in EXPECTED_QUERY_FRAGMENTS:
        fail(f"unexpected alert uid: {uid!r}")
    exact(rule_map.get("condition"), "C", f"{uid}: condition")

    data = rule_map.get("data")
    if not isinstance(data, list) or len(data) != 2:
        fail(f"{uid}: expected exactly two data nodes")
    nodes = {}
    for node in data:
        node_map = mapping(node, f"{uid}: data node")
        ref_id = node_map.get("refId")
        if ref_id in nodes:
            fail(f"{uid}: duplicate data refId {ref_id!r}")
        nodes[ref_id] = node_map
    if set(nodes) != {"A", "C"}:
        fail(f"{uid}: data refIds must be exactly A and C")

    source = nodes["A"]
    exact(source.get("datasourceUid"), "hikari-prometheus", f"{uid}: source datasourceUid")
    source_model = mapping(source.get("model"), f"{uid}: source model")
    exact(source_model.get("refId"), "A", f"{uid}: source model refId")
    source_datasource = mapping(source_model.get("datasource"), f"{uid}: source model datasource")
    exact(source_datasource.get("type"), "prometheus", f"{uid}: source model type")
    exact(source_datasource.get("uid"), "hikari-prometheus", f"{uid}: source model uid")
    expression = source_model.get("expr")
    if not isinstance(expression, str) or not expression.strip():
        fail(f"{uid}: source PromQL expression is empty")
    if uid != "hikari-exporter-unavailable" and re.search(r"\bbool\b", expression):
        fail(f"{uid}: bool comparisons cannot filter set operators")
    for fragment in EXPECTED_QUERY_FRAGMENTS[uid]:
        if fragment not in expression:
            fail(f"{uid}: source PromQL is missing {fragment!r}")

    threshold = nodes["C"]
    exact(threshold.get("datasourceUid"), "__expr__", f"{uid}: threshold datasourceUid")
    threshold_model = mapping(threshold.get("model"), f"{uid}: threshold model")
    exact(threshold_model.get("refId"), "C", f"{uid}: threshold model refId")
    exact(threshold_model.get("type"), "threshold", f"{uid}: threshold model type")
    exact(threshold_model.get("expression"), "A", f"{uid}: threshold expression")
    threshold_datasource = mapping(
        threshold_model.get("datasource"), f"{uid}: threshold model datasource"
    )
    exact(threshold_datasource.get("type"), "__expr__", f"{uid}: threshold model type")
    exact(threshold_datasource.get("uid"), "__expr__", f"{uid}: threshold model uid")
    conditions = threshold_model.get("conditions")
    if not isinstance(conditions, list) or len(conditions) != 1:
        fail(f"{uid}: expected one threshold condition")
    condition = mapping(conditions[0], f"{uid}: threshold condition")
    evaluator = mapping(condition.get("evaluator"), f"{uid}: evaluator")
    exact(evaluator.get("type"), "gt", f"{uid}: evaluator type")
    exact(evaluator.get("params"), [0], f"{uid}: evaluator params")
    query = mapping(condition.get("query"), f"{uid}: threshold query")
    exact(query.get("params"), ["A"], f"{uid}: threshold query params")
    return uid, expression


def load_rules(path: Path) -> list[tuple[str, str]]:
    try:
        payload = yaml.load(path.read_text(encoding="utf-8"), Loader=UniqueKeyLoader)
    except yaml.YAMLError as exc:
        fail(f"{path}: invalid YAML: {exc}")
    document = mapping(payload, "alert provisioning")
    exact(document.get("apiVersion"), 1, "alert provisioning apiVersion")
    groups = document.get("groups")
    if not isinstance(groups, list) or len(groups) != 1:
        fail("alert provisioning must contain one group")
    group = mapping(groups[0], "alert group")
    rules = group.get("rules")
    if not isinstance(rules, list) or len(rules) != len(EXPECTED_QUERY_FRAGMENTS):
        fail(f"alert provisioning must contain {len(EXPECTED_QUERY_FRAGMENTS)} rules")
    validated = [validate_rule(rule) for rule in rules]
    uids = [uid for uid, _ in validated]
    if len(set(uids)) != len(uids) or set(uids) != set(EXPECTED_QUERY_FRAGMENTS):
        fail("alert provisioning UID set must contain each managed Hikari alert exactly once")
    return validated


def write_prometheus_rules(path: Path, expressions: list[tuple[str, str]]) -> None:
    rules = []
    for uid, expression in expressions:
        record = "hikari_alert_validation_" + re.sub(r"[^a-z0-9]+", "_", uid).strip("_")
        rules.append({"record": record, "expr": expression})
    payload = {"groups": [{"name": "hikari-grafana-alert-expressions", "rules": rules}]}
    path.write_text(yaml.safe_dump(payload, sort_keys=False), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("alert_rules", type=Path)
    parser.add_argument("--prometheus-rules-output", type=Path)
    args = parser.parse_args()
    expressions = load_rules(args.alert_rules)
    if args.prometheus_rules_output:
        write_prometheus_rules(args.prometheus_rules_output, expressions)


if __name__ == "__main__":
    main()
