use std::fs;
use std::path::{Path, PathBuf};

const MAX_RUST_SOURCE_LINES: usize = 3050;
const IGNORE_DIRS: &[&str] = &["target", ".git"];
const EXCEPTIONS: &[(&str, usize, &str)] = &[
    (
        "src/models.rs",
        3200,
        "Alert center semantic DTO growth plus the pressure and snapshot contracts both extend the shared model surface while the broader extraction pass remains pending.",
    ),
    (
        "src/server/tests/admin_users_and_tokens.rs",
        3760,
        "Admin user HTTP/SSE integration coverage still lives in the legacy consolidated server test file while active-user rollup coverage, rankings six-tab/IP payload assertions, and the new analysis pressure contract converge before a broader extraction pass.",
    ),
    (
        "src/tests/jobs_and_request_log_retention.rs",
        3100,
        "Request-log retention and scheduled-job regression coverage still lives in the consolidated jobs/request-log suite while the remaining extraction work lands in follow-up slices.",
    ),
    (
        "src/store/key_store_request_logs_and_dashboard.rs",
        3160,
        "Request-log persistence, dashboard rollups, and the new request-log cursor filters still live in the legacy shared store module while the follow-up extraction pass is pending.",
    ),
    (
        "src/server/tests/api_keys_and_registration.rs",
        3310,
        "User auth/profile integration coverage now also carries the dedicated billing summary endpoint contract, related recharge/user-console assertions, and AppState admission wiring while the legacy consolidated server test file still awaits a broader extraction pass.",
    ),
    (
        "src/server/tests/tavily_http_search.rs",
        3250,
        "HTTP search integration coverage now also carries the upstream privacy project-id modes, rollout/header contracts, and the business-call reservation regression path while the legacy consolidated server test file still awaits a broader extraction pass.",
    ),
    (
        "src/store/key_store_bootstrap.rs",
        3275,
        "The bootstrap schema module now also carries upstream reconciliation polling and persisted HA GC state migrations while the broader store-schema split remains pending.",
    ),
    (
        "src/store/key_store_ha.rs",
        3400,
        "HA outbox retention, peer health, cursor-gap markers, and bounded online cleanup remain together in the channel store module while the HA store extraction pass is pending.",
    ),
    (
        "src/server/schedulers.rs",
        3120,
        "The scheduler retains the consolidated durable-job lifecycle while request-scoped remote-attempt admission is rolled out across existing maintenance job types; task-dispatch extraction remains a separate behavior-preserving change.",
    ),
    (
        "src/tavily_proxy/proxy_quota_sync_and_jobs.rs",
        3250,
        "Reconciliation execution and quota synchronization remain together while per-key cooldown, request-scoped admission, and typed Research poll outcomes converge; extracting the remaining run phases is a separate behavior-preserving change.",
    ),
    (
        "src/tests/upstream_reconciliation.rs",
        3500,
        "The consolidated upstream reconciliation integration suite carries the multi-key observation, per-key cooldown, and continuation-fencing coverage while a broader test extraction remains a separate behavior-preserving change.",
    ),
    (
        "src/store/sqlite_runtime.rs",
        3550,
        "The runtime owns the shared pool, operation budgeting, transaction guards, admission state, workload aggregation, and the bounded reconciliation read session; cooperative query cleanup remains isolated in its dedicated child module.",
    ),
];

fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read_dir {}: {err}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("read_dir entry {}: {err}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            if path.components().any(|component| {
                IGNORE_DIRS.contains(&component.as_os_str().to_string_lossy().as_ref())
            }) {
                continue;
            }
            visit(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .lines()
        .count()
}

fn resolve_budget(relative: &Path) -> (usize, Option<&'static str>) {
    let relative = relative.to_string_lossy().replace('\\', "/");
    EXCEPTIONS
        .iter()
        .find(|(path, _, _)| *path == relative)
        .map(|(_, max, reason)| (*max, Some(*reason)))
        .unwrap_or((MAX_RUST_SOURCE_LINES, None))
}

#[test]
fn rust_source_files_stay_within_line_budget() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    visit(&repo_root.join("src"), &mut files);
    visit(&repo_root.join("tests"), &mut files);
    files.sort();

    let over_budget: Vec<String> = files
        .into_iter()
        .filter_map(|path| {
            let lines = count_lines(&path);
            let relative = path.strip_prefix(&repo_root).unwrap_or(&path);
            let (max, reason) = resolve_budget(relative);
            (lines > max).then(|| {
                let reason = reason
                    .map(|value| format!(" | reason: {value}"))
                    .unwrap_or_default();
                format!(
                    "{}: {} lines > {}{}",
                    relative.display(),
                    lines,
                    max,
                    reason
                )
            })
        })
        .collect();

    assert!(
        over_budget.is_empty(),
        "Rust source file line budget exceeded:\n{}",
        over_budget.join("\n")
    );
}
