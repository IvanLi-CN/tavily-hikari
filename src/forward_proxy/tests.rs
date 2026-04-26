#[cfg(test)]
mod tests {
    use super::*;
    use crate::tavily_proxy::{TavilyProxy, TavilyProxyOptions};
    use crate::LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn derive_probe_url_uses_public_probe_endpoint() {
        let upstream = Url::parse("http://127.0.0.1:30014/mcp").expect("parse upstream");
        let probe = derive_probe_url(&upstream);

        assert_eq!(probe.as_str(), "http://example.com/");
    }

    #[test]
    fn parse_proxy_urls_from_subscription_body_ignores_structured_yaml_configs() {
        let body = r#"
proxies:
  - name: hinet-reality
    type: vless
    server: hinet-ep.707979.xyz
    port: 53842
rule-providers:
  sample:
    type: http
    url: https://example.com/rules.yaml
"#;

        assert!(parse_proxy_urls_from_subscription_body(body).is_empty());
        assert!(subscription_body_uses_unsupported_structure(body));
    }

    #[test]
    fn build_vless_xray_outbound_preserves_reality_settings() {
        let outbound = build_vless_xray_outbound("vless://0688fa59-e971-4278-8c03-4b35821a71dc@hklb-ep.707979.xyz:53842?encryption=none&security=reality&type=tcp&sni=public.sn.files.1drv.com&fp=chrome&pbk=6cJN5zHglyIywI_ZnsC7xW6lD1IO9gkHSvw6uvULCWQ&sid=61446ca92a46cdc7&flow=xtls-rprx-vision#Ivan-hkl-vless-vision").expect("build outbound");
        let stream = outbound
            .get("streamSettings")
            .and_then(Value::as_object)
            .expect("stream settings");
        assert_eq!(
            stream.get("security").and_then(Value::as_str),
            Some("reality")
        );

        let reality = stream
            .get("realitySettings")
            .and_then(Value::as_object)
            .expect("reality settings");
        assert_eq!(
            reality.get("serverName").and_then(Value::as_str),
            Some("public.sn.files.1drv.com")
        );
        assert_eq!(
            reality.get("fingerprint").and_then(Value::as_str),
            Some("chrome")
        );
        assert_eq!(
            reality.get("publicKey").and_then(Value::as_str),
            Some("6cJN5zHglyIywI_ZnsC7xW6lD1IO9gkHSvw6uvULCWQ")
        );
        assert_eq!(
            reality.get("shortId").and_then(Value::as_str),
            Some("61446ca92a46cdc7")
        );
    }

    #[test]
    fn parse_vless_forward_proxy_decodes_percent_encoded_display_name_once() {
        let parsed = parse_vless_forward_proxy(
            "vless://0688fa59-e971-4278-8c03-4b35821a71dc@example.com:443?encryption=none#%E9%A6%99%E6%B8%AF%20%F0%9F%87%AD%F0%9F%87%B0",
        )
        .expect("parse vless");

        assert_eq!(parsed.display_name, "香港 🇭🇰");
    }

    #[test]
    fn parse_trojan_forward_proxy_falls_back_when_fragment_decodes_to_blank() {
        let parsed =
            parse_trojan_forward_proxy("trojan://secret@example.com:8443?security=tls#%20%20")
                .expect("parse trojan");

        assert_eq!(parsed.display_name, "example.com:8443");
    }

    #[test]
    fn parse_vless_forward_proxy_keeps_lossy_fragment_for_invalid_percent_encoding() {
        let parsed = parse_vless_forward_proxy(
            "vless://0688fa59-e971-4278-8c03-4b35821a71dc@example.com:443?encryption=none#broken%ZZname",
        )
        .expect("parse vless");

        assert_eq!(parsed.display_name, "broken%ZZname");
    }

    #[test]
    fn endpoint_host_prefers_share_link_host_for_xray_routes() {
        let endpoint = ForwardProxyEndpoint {
            key: "vless://example".to_string(),
            source: FORWARD_PROXY_SOURCE_MANUAL.to_string(),
            display_name: "example".to_string(),
            protocol: ForwardProxyProtocol::Vless,
            endpoint_url: Some(
                Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route"),
            ),
            raw_url: Some(
                "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk"
                    .to_string(),
            ),
            manual_present: true,
            subscription_sources: BTreeSet::new(),
            uses_local_relay: false,
            relay_handle: None,
        };

        assert_eq!(endpoint_host(&endpoint).as_deref(), Some("1.1.1.1"));
    }

    #[test]
    fn endpoint_host_keeps_local_listener_for_non_xray_routes() {
        let endpoint = ForwardProxyEndpoint {
            key: "http://127.0.0.1:8080".to_string(),
            source: FORWARD_PROXY_SOURCE_MANUAL.to_string(),
            display_name: "local".to_string(),
            protocol: ForwardProxyProtocol::Http,
            endpoint_url: Some(Url::parse("http://127.0.0.1:8080").expect("parse http url")),
            raw_url: Some("http://example.com:8080".to_string()),
            manual_present: true,
            subscription_sources: BTreeSet::new(),
            uses_local_relay: false,
            relay_handle: None,
        };

        assert_eq!(endpoint_host(&endpoint).as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn parse_egress_socks5_url_requires_supported_scheme_and_explicit_port() {
        assert!(
            parse_egress_socks5_url("socks5h://user:pass@127.0.0.1:1080").is_some(),
            "complete socks5h URLs should remain valid",
        );
        assert!(
            parse_egress_socks5_url("socks5://127.0.0.1").is_none(),
            "missing ports should be rejected for egress URLs",
        );
        assert!(
            parse_egress_socks5_url("socks5h://user:pass@127").is_none(),
            "hostname-only values without an explicit port should be rejected",
        );
        assert!(
            parse_egress_socks5_url("http://127.0.0.1:1080").is_none(),
            "non-SOCKS egress URLs should be rejected",
        );
    }

    #[test]
    fn subscription_refresh_preserves_overlapping_manual_and_subscription_sources() {
        let subscription_url = "https://subscription.example.com/feed".to_string();
        let endpoint_url = "http://198.51.100.8:8080".to_string();
        let settings = ForwardProxySettings {
            proxy_urls: vec![endpoint_url.clone()],
            subscription_urls: vec![subscription_url.clone()],
            subscription_update_interval_secs: 3600,
            insert_direct: false,

            egress_socks5_enabled: false,
            egress_socks5_url: String::new(),
        };
        let mut manager = ForwardProxyManager::new(settings.clone(), Vec::new());
        let fetched = HashMap::from([(subscription_url.clone(), vec![endpoint_url.clone()])]);

        manager.apply_subscription_refresh(&fetched);

        let endpoint = manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == endpoint_url)
            .expect("overlapping endpoint present");
        assert!(endpoint.manual_present);
        assert_eq!(
            endpoint.subscription_sources,
            BTreeSet::from([subscription_url.clone()])
        );
        assert_eq!(endpoint.source, FORWARD_PROXY_SOURCE_MANUAL);

        manager.apply_incremental_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                ..settings
            },
            &HashMap::new(),
        );

        let endpoint = manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == endpoint_url)
            .expect("subscription-backed endpoint should remain after manual removal");
        assert!(!endpoint.manual_present);
        assert_eq!(
            endpoint.subscription_sources,
            BTreeSet::from([subscription_url])
        );
        assert_eq!(endpoint.source, FORWARD_PROXY_SOURCE_SUBSCRIPTION);
    }

    #[test]
    fn incremental_subscription_save_updates_refresh_timestamp() {
        let subscription_url = "https://subscription.example.com/feed".to_string();
        let endpoint_url = "http://198.51.100.8:8080".to_string();
        let mut manager = ForwardProxyManager::new(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            },
            Vec::new(),
        );

        manager.apply_incremental_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: vec![subscription_url.clone()],
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            },
            &HashMap::from([(subscription_url, vec![endpoint_url])]),
        );

        assert!(manager.last_subscription_refresh_at.is_some());
        assert!(!manager.should_refresh_subscriptions());
    }

    fn temp_runtime_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&dir).expect("create temp runtime dir");
        dir
    }

    fn write_fake_xray_binary(prefix: &str) -> String {
        write_fake_xray_binary_with_api_failure(prefix, None)
    }

    fn write_fake_xray_binary_with_api_failure(prefix: &str, fail_command: Option<&str>) -> String {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-fake-xray-{}-{}.py",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let script = r#"#!/usr/bin/env python3
import json
import os
import signal
import socket
import sys
import threading
import time
from pathlib import Path

FAIL_COMMAND = "__FAIL_COMMAND__"

def state_path_for_server(server: str) -> Path:
    port = server.rsplit(":", 1)[1]
    return Path(f"/tmp/fake-xray-{port}.json")

def load_json(path: Path):
    if not path.exists():
        return {"inbounds": {}, "outbounds": {}, "rules": {}}
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)

def save_json(path: Path, data):
    tmp = path.with_suffix(".tmp")
    with tmp.open("w", encoding="utf-8") as f:
        json.dump(data, f)
    tmp.replace(path)

class DummyListener:
    def __init__(self, host: str, port: int):
        self._stop = threading.Event()
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind((host, port))
        self._sock.listen()
        self._sock.settimeout(0.2)
        self._thread = threading.Thread(target=self._loop, daemon=True)
        self._thread.start()

    def _loop(self):
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            try:
                conn.settimeout(0.2)
                greeting = conn.recv(3)
                if greeting == b"\x05\x01\x00":
                    conn.sendall(b"\x05\x00")
            except OSError:
                pass
            finally:
                try:
                    conn.close()
                except OSError:
                    pass

    def close(self):
        self._stop.set()
        try:
            self._sock.close()
        except OSError:
            pass
        self._thread.join(timeout=1)

def run_mode(config_path: str) -> int:
    config = load_json(Path(config_path))
    listen = config["api"]["listen"]
    host, port = listen.rsplit(":", 1)
    port = int(port)
    state_path = state_path_for_server(listen)
    save_json(state_path, {"inbounds": {}, "outbounds": {}, "rules": {}})

    api_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    api_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    api_sock.bind((host, port))
    api_sock.listen()
    api_sock.settimeout(0.2)

    listeners = {}
    stop = False

    def handle_signal(_signum, _frame):
        nonlocal stop
        stop = True

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    try:
        while not stop:
            try:
                conn, _ = api_sock.accept()
                conn.close()
            except socket.timeout:
                pass
            except OSError:
                break

            state = load_json(state_path)
            desired = {
                tag: int(item["port"])
                for tag, item in state.get("inbounds", {}).items()
            }
            for tag, listener in list(listeners.items()):
                if tag not in desired:
                    listener.close()
                    listeners.pop(tag, None)
            for tag, inbound_port in desired.items():
                if tag not in listeners:
                    listeners[tag] = DummyListener("127.0.0.1", inbound_port)
            time.sleep(0.05)
    finally:
        for listener in listeners.values():
            listener.close()
        try:
            api_sock.close()
        except OSError:
            pass
        try:
            state_path.unlink()
        except FileNotFoundError:
            pass
    return 0

def collect_json_args(args):
    return [Path(arg) for arg in args if arg.endswith(".json")]

def parse_server(args):
    server = "127.0.0.1:8080"
    positionals = []
    skip = False
    for index, arg in enumerate(args):
        if skip:
            skip = False
            continue
        if arg.startswith("--server="):
            server = arg.split("=", 1)[1]
            continue
        if arg in ("--server", "-s"):
            server = args[index + 1]
            skip = True
            continue
        if arg in ("--timeout", "-t", "-append"):
            if arg in ("--timeout", "-t"):
                skip = True
            continue
        positionals.append(arg)
    return server, positionals

def api_mode(command: str, args) -> int:
    if FAIL_COMMAND and command == FAIL_COMMAND:
        print(f"forced api failure for {command}", file=sys.stderr)
        return 1
    server, positionals = parse_server(args)
    state_path = state_path_for_server(server)
    state = load_json(state_path)

    if command == "adi":
        for config_path in collect_json_args(positionals):
            config = load_json(config_path)
            for inbound in config.get("inbounds", []):
                state.setdefault("inbounds", {})[inbound["tag"]] = {
                    "port": inbound["port"]
                }
    elif command == "ado":
        for config_path in collect_json_args(positionals):
            config = load_json(config_path)
            for outbound in config.get("outbounds", []):
                state.setdefault("outbounds", {})[outbound["tag"]] = True
    elif command == "adrules":
        for config_path in collect_json_args(positionals):
            config = load_json(config_path)
            for rule in config.get("routing", {}).get("rules", []):
                state.setdefault("rules", {})[rule["ruleTag"]] = rule.get("outboundTag")
    elif command == "rmi":
        for tag in positionals:
            state.setdefault("inbounds", {}).pop(tag, None)
    elif command == "rmo":
        for tag in positionals:
            state.setdefault("outbounds", {}).pop(tag, None)
    elif command == "rmrules":
        for tag in positionals:
            state.setdefault("rules", {}).pop(tag, None)

    save_json(state_path, state)
    return 0

def main():
    argv = sys.argv[1:]
    if not argv:
        return 1
    if argv[0] == "run":
        config_path = argv[argv.index("-c") + 1]
        return run_mode(config_path)
    if argv[0] == "api":
        return api_mode(argv[1], argv[2:])
    return 1

if __name__ == "__main__":
    raise SystemExit(main())
"#
        .replace("__FAIL_COMMAND__", fail_command.unwrap_or(""));
        fs::write(&path, script).expect("write fake xray script");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path)
                .expect("fake xray metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).expect("chmod fake xray");
        }
        path.to_string_lossy().to_string()
    }

    fn sample_vless_share_link(host: &str, label: &str) -> String {
        format!(
            "vless://0688fa59-e971-4278-8c03-4b35821a71dc@{host}:443?encryption=none#{}",
            urlencoding::encode(label)
        )
    }

    fn subscription_vless_endpoint(key: &str, host: &str, label: &str) -> ForwardProxyEndpoint {
        ForwardProxyEndpoint::new_subscription(
            key.to_string(),
            label.to_string(),
            ForwardProxyProtocol::Vless,
            None,
            Some(sample_vless_share_link(host, label)),
            "https://subscription.example.com/feed".to_string(),
        )
    }

    #[test]
    fn reserved_local_port_keeps_port_bound_until_release() {
        let mut reservation = reserve_unused_local_port().expect("reserve loopback port");
        let port = reservation.port();
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
            "reserved port should stay bound until release"
        );

        reservation.release();

        let rebound = (0..20)
            .find_map(
                |_| match std::net::TcpListener::bind(("127.0.0.1", port)) {
                    Ok(listener) => Some(listener),
                    Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        None
                    }
                    Err(err) => panic!("released port should be reusable: {err}"),
                },
            )
            .expect("released port should become reusable");
        drop(rebound);
    }

    #[tokio::test]
    async fn wait_for_local_socks_ready_rejects_unrelated_listener() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind unrelated listener");
        let port = listener.local_addr().expect("listener addr").port();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.expect("accept unrelated client");
                drop(socket);
            }
        });

        let err = wait_for_local_socks_ready(port, Duration::from_millis(250))
            .await
            .expect_err("plain listener should not satisfy socks readiness");
        assert!(
            err.to_string()
                .contains("xray local socks endpoint was not ready in time"),
            "unexpected error: {err}"
        );

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn wait_for_local_socks_ready_accepts_socks_handshake() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind socks listener");
        let port = listener.local_addr().expect("listener addr").port();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept socks client");
                tokio::spawn(async move {
                    let mut greeting = [0_u8; 3];
                    socket
                        .read_exact(&mut greeting)
                        .await
                        .expect("read socks greeting");
                    assert_eq!(greeting, [0x05, 0x01, 0x00]);
                    socket
                        .write_all(&[0x05, 0x00])
                        .await
                        .expect("write socks greeting response");
                });
            }
        });

        wait_for_local_socks_ready(port, Duration::from_secs(1))
            .await
            .expect("socks handshake should mark relay ready");

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn xray_supervisor_reuses_single_shared_process_and_hot_swaps_changed_handles() {
        let runtime_dir = temp_runtime_dir("shared-xray-hot-swap");
        let mut supervisor =
            XraySupervisor::new(write_fake_xray_binary("shared-xray-hot-swap"), runtime_dir);

        let mut initial = vec![
            subscription_vless_endpoint("node-a", "a.example.com", "Alpha"),
            subscription_vless_endpoint("node-b", "b.example.com", "Bravo"),
        ];
        supervisor
            .sync_endpoints(&mut initial, None)
            .await
            .expect("initial sync");
        let first_snapshot = supervisor.debug_snapshot().await;
        let first_pid = first_snapshot
            .shared_pid
            .expect("shared pid after first sync");
        let initial_alpha_url = initial[0]
            .endpoint_url
            .clone()
            .expect("alpha endpoint url after first sync");
        let initial_bravo_url = initial[1]
            .endpoint_url
            .clone()
            .expect("bravo endpoint url after first sync");
        assert_eq!(first_snapshot.active_endpoint_handles, 2);
        assert_eq!(first_snapshot.total_handles, 2);
        assert_eq!(first_snapshot.retiring_handles, 0);

        let mut updated = vec![
            subscription_vless_endpoint("node-a", "a.example.com", "Alpha"),
            subscription_vless_endpoint("node-b", "c.example.com", "Charlie"),
        ];
        supervisor
            .sync_endpoints(&mut updated, None)
            .await
            .expect("updated sync");
        let second_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(second_snapshot.shared_pid, Some(first_pid));
        assert_eq!(second_snapshot.active_endpoint_handles, 2);
        assert_eq!(second_snapshot.total_handles, 2);
        assert_eq!(second_snapshot.retiring_handles, 0);
        assert_eq!(updated[0].endpoint_url.as_ref(), Some(&initial_alpha_url));
        assert_ne!(updated[1].endpoint_url.as_ref(), Some(&initial_bravo_url));

        supervisor.shutdown_all().await;
    }

    #[tokio::test]
    async fn xray_supervisor_drains_retired_handles_until_last_lease_releases() {
        let runtime_dir = temp_runtime_dir("shared-xray-drain");
        let mut supervisor =
            XraySupervisor::new(write_fake_xray_binary("shared-xray-drain"), runtime_dir);

        let mut initial = vec![subscription_vless_endpoint(
            "node-a",
            "a.example.com",
            "Alpha",
        )];
        supervisor
            .sync_endpoints(&mut initial, None)
            .await
            .expect("initial sync");
        let old_url = initial[0]
            .endpoint_url
            .clone()
            .expect("old endpoint url after sync");
        let lease_id = supervisor
            .acquire_relay_lease_by_url(Some(&old_url))
            .await
            .expect("old relay lease id");

        let mut changed = vec![subscription_vless_endpoint(
            "node-a",
            "changed.example.com",
            "Alpha New",
        )];
        supervisor
            .sync_endpoints(&mut changed, None)
            .await
            .expect("changed sync");
        let draining_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(draining_snapshot.active_endpoint_handles, 1);
        assert_eq!(draining_snapshot.total_handles, 2);
        assert_eq!(draining_snapshot.retiring_handles, 1);
        assert_ne!(changed[0].endpoint_url.as_ref(), Some(&old_url));

        supervisor.release_relay_lease(&lease_id).await;
        let settled_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(settled_snapshot.active_endpoint_handles, 1);
        assert_eq!(settled_snapshot.total_handles, 1);
        assert_eq!(settled_snapshot.retiring_handles, 0);

        supervisor.shutdown_all().await;
    }

    #[tokio::test]
    async fn xray_supervisor_retiring_handle_stays_leaseable_for_selected_plan() {
        let runtime_dir = temp_runtime_dir("shared-xray-plan-drain");
        let supervisor = Arc::new(Mutex::new(XraySupervisor::new(
            write_fake_xray_binary("shared-xray-plan-drain"),
            runtime_dir,
        )));

        let selected = {
            let mut locked = supervisor.lock().await;
            let mut initial = vec![subscription_vless_endpoint(
                "node-a",
                "a.example.com",
                "Alpha",
            )];
            locked
                .sync_endpoints(&mut initial, None)
                .await
                .expect("initial sync");
            SelectedForwardProxy::from_endpoint(&initial[0])
        };

        {
            let mut locked = supervisor.lock().await;
            let mut changed = vec![subscription_vless_endpoint(
                "node-a",
                "changed.example.com",
                "Alpha New",
            )];
            locked
                .sync_endpoints(&mut changed, None)
                .await
                .expect("changed sync");
            let draining_snapshot = locked.debug_snapshot().await;
            assert_eq!(draining_snapshot.total_handles, 2);
            assert_eq!(draining_snapshot.retiring_handles, 1);
        }

        let lease =
            ForwardProxyRelayLease::acquire_for_selection(Arc::clone(&supervisor), &selected)
                .await
                .expect("selected plan should still acquire a lease on retiring handle");
        let draining_snapshot = supervisor.lock().await.debug_snapshot().await;
        assert_eq!(draining_snapshot.total_handles, 2);
        assert_eq!(draining_snapshot.retiring_handles, 1);

        lease.release().await;
        drop(selected);
        let settled_snapshot = {
            let mut locked = supervisor.lock().await;
            locked.reap_retired_handles_now().await;
            locked.debug_snapshot().await
        };
        assert_eq!(settled_snapshot.total_handles, 1);
        assert_eq!(settled_snapshot.retiring_handles, 0);

        supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn forward_proxy_relay_lease_acquire_waits_for_supervisor_mutex_reset() {
        let runtime_dir = temp_runtime_dir("shared-xray-lease-fast-path");
        let supervisor = Arc::new(Mutex::new(XraySupervisor::new(
            write_fake_xray_binary("shared-xray-lease-fast-path"),
            runtime_dir,
        )));

        let selected = {
            let mut locked = supervisor.lock().await;
            let mut endpoints = vec![subscription_vless_endpoint(
                "node-a",
                "lease.example.com",
                "Lease Node",
            )];
            locked
                .sync_endpoints(&mut endpoints, None)
                .await
                .expect("sync endpoints");
            SelectedForwardProxy::from_endpoint(&endpoints[0])
        };

        let held_guard = supervisor.lock().await;
        let held_started = Instant::now();
        let started = Instant::now();
        let acquire_task = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let selected = selected.clone();
            async move { ForwardProxyRelayLease::acquire_for_selection(supervisor, &selected).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !acquire_task.is_finished(),
            "lease acquisition should wait for the supervisor mutex before granting the relay"
        );
        drop(held_guard);
        let lease = acquire_task
            .await
            .expect("lease acquisition task should complete")
            .expect("lease acquisition should use the selected relay handle");
        assert!(
            held_started.elapsed() >= Duration::from_millis(50),
            "expected supervisor mutex wait before acquiring the relay lease"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(50),
            "lease acquisition should have waited for the held supervisor mutex"
        );

        lease.release().await;
        supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn tavily_proxy_send_plan_reaps_retired_handles_after_plan_drop() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-plan-drop");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = TavilyProxy::with_options(
            Vec::<String>::new(),
            "http://127.0.0.1:9/mcp",
            db_path
                .to_str()
                .expect("database path should be valid utf-8"),
            TavilyProxyOptions {
                xray_binary: write_fake_xray_binary("proxy-shared-xray-plan-drop"),
                xray_runtime_dir: runtime_dir,
                forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                    .expect("valid trace url"),
                low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
            },
        )
        .await
        .expect("create proxy with fake xray");

        let stale_candidate = {
            let mut supervisor = proxy.xray_supervisor.lock().await;
            let mut initial = vec![subscription_vless_endpoint(
                "node-a",
                "a.example.com",
                "Alpha",
            )];
            supervisor
                .sync_endpoints(&mut initial, None)
                .await
                .expect("initial sync");
            let stale = SelectedForwardProxy::from_endpoint(&initial[0]);
            let mut changed = vec![subscription_vless_endpoint(
                "node-a",
                "changed.example.com",
                "Alpha New",
            )];
            supervisor
                .sync_endpoints(&mut changed, None)
                .await
                .expect("changed sync");
            let snapshot = supervisor.debug_snapshot().await;
            assert_eq!(snapshot.total_handles, 2);
            assert_eq!(snapshot.retiring_handles, 1);
            stale
        };

        proxy
            .send_with_forward_proxy_plan(
                "subject",
                None,
                "request",
                vec![stale_candidate],
                |client| client.get("http://127.0.0.1:9/"),
            )
            .await
            .expect_err("closed upstream should fail");

        let snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert_eq!(snapshot.active_endpoint_handles, 1);
        assert_eq!(snapshot.total_handles, 1);
        assert_eq!(snapshot.retiring_handles, 0);

        proxy.xray_supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn xray_supervisor_validation_handles_cleanup_idle_shared_process() {
        let runtime_dir = temp_runtime_dir("shared-xray-validate-temp");
        let mut supervisor = XraySupervisor::new(
            write_fake_xray_binary("shared-xray-validate-temp"),
            runtime_dir,
        );
        let endpoint = subscription_vless_endpoint("validate-node", "validate.example.com", "Temp");

        let resolved = supervisor
            .resolve_validation_endpoint(&endpoint, None)
            .await
            .expect("resolve validation endpoint");
        let lease_id = supervisor
            .acquire_relay_lease_by_url(resolved.endpoint_url.as_ref())
            .await
            .expect("temporary validation lease");
        assert!(resolved.uses_local_relay);
        assert!(resolved.endpoint_url.is_some());

        let active_snapshot = supervisor.debug_snapshot().await;
        assert!(active_snapshot.shared_pid.is_some());
        assert_eq!(active_snapshot.active_endpoint_handles, 0);
        assert_eq!(active_snapshot.total_handles, 1);
        assert_eq!(active_snapshot.retiring_handles, 1);

        supervisor.release_relay_lease(&lease_id).await;
        let cleaned_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(cleaned_snapshot.shared_pid, None);
        assert_eq!(cleaned_snapshot.active_endpoint_handles, 0);
        assert_eq!(cleaned_snapshot.total_handles, 0);
        assert_eq!(cleaned_snapshot.retiring_handles, 0);
        assert!(
            cleaned_snapshot.runtime_files.is_empty(),
            "temporary validation should not leave runtime files behind: {:?}",
            cleaned_snapshot.runtime_files
        );
    }

    #[tokio::test]
    async fn xray_supervisor_failed_temp_handle_creation_cleans_up_shared_process() {
        let runtime_dir = temp_runtime_dir("shared-xray-temp-failure");
        let mut supervisor = XraySupervisor::new(
            write_fake_xray_binary_with_api_failure("shared-xray-temp-failure", Some("ado")),
            runtime_dir,
        );
        let endpoint = subscription_vless_endpoint("validate-node", "validate.example.com", "Temp");

        supervisor
            .resolve_validation_endpoint(&endpoint, None)
            .await
            .expect_err("failing xray api should abort temp handle creation");

        let snapshot = supervisor.debug_snapshot().await;
        assert_eq!(snapshot.shared_pid, None);
        assert_eq!(snapshot.active_endpoint_handles, 0);
        assert_eq!(snapshot.total_handles, 0);
        assert_eq!(snapshot.retiring_handles, 0);
        assert!(
            snapshot.runtime_files.is_empty(),
            "failed temp handle creation should not leave runtime files behind: {:?}",
            snapshot.runtime_files
        );
    }

    #[tokio::test]
    async fn validation_endpoint_holds_first_lease_before_reap() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-validation-lease");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = TavilyProxy::with_options(
            Vec::<String>::new(),
            "http://127.0.0.1:9/mcp",
            db_path
                .to_str()
                .expect("database path should be valid utf-8"),
            TavilyProxyOptions {
                xray_binary: write_fake_xray_binary("proxy-shared-xray-validation-lease"),
                xray_runtime_dir: runtime_dir,
                forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                    .expect("valid trace url"),
                low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
            },
        )
        .await
        .expect("create proxy with fake xray");
        let endpoint =
            subscription_vless_endpoint("validate-node", "validate.example.com", "Validate");

        let (_resolved, relay_lease) = proxy
            .resolve_forward_proxy_validation_endpoint(&endpoint)
            .await
            .expect("resolve validation endpoint with held lease");

        {
            let mut supervisor = proxy.xray_supervisor.lock().await;
            supervisor.reap_retired_handles_now().await;
            let snapshot = supervisor.debug_snapshot().await;
            assert!(snapshot.shared_pid.is_some());
            assert_eq!(snapshot.total_handles, 1);
            assert_eq!(snapshot.retiring_handles, 1);
        }

        relay_lease.release().await;
        let snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert_eq!(snapshot.shared_pid, None);
        assert_eq!(snapshot.total_handles, 0);
        assert_eq!(snapshot.retiring_handles, 0);
    }

    #[tokio::test]
    async fn tavily_proxy_probe_failure_releases_validation_lease() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-probe-failure");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = TavilyProxy::with_options(
            Vec::<String>::new(),
            "http://127.0.0.1:9/mcp",
            db_path
                .to_str()
                .expect("database path should be valid utf-8"),
            TavilyProxyOptions {
                xray_binary: write_fake_xray_binary("proxy-shared-xray-probe-failure"),
                xray_runtime_dir: runtime_dir,
                forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                    .expect("valid trace url"),
                low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
            },
        )
        .await
        .expect("create proxy with fake xray");
        let endpoint =
            subscription_vless_endpoint("validate-node", "validate.example.com", "Validate");
        let probe_url = Url::parse("http://127.0.0.1:9/").expect("valid probe url");

        proxy
            .probe_forward_proxy_endpoint(&endpoint, Duration::from_millis(100), &probe_url, None)
            .await
            .expect_err("probe should fail against closed probe port");

        let snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert_eq!(snapshot.shared_pid, None);
        assert_eq!(snapshot.active_endpoint_handles, 0);
        assert_eq!(snapshot.total_handles, 0);
        assert_eq!(snapshot.retiring_handles, 0);
        assert!(
            snapshot.runtime_files.is_empty(),
            "failed validation probe should not leak runtime files: {:?}",
            snapshot.runtime_files
        );
    }

    #[tokio::test]
    async fn xray_supervisor_clears_cached_relay_urls_after_shared_process_exit() {
        let runtime_dir = temp_runtime_dir("shared-xray-dead-process");
        let mut supervisor = XraySupervisor::new(
            write_fake_xray_binary("shared-xray-dead-process"),
            runtime_dir,
        );
        let mut endpoints = vec![subscription_vless_endpoint(
            "node-a",
            "dead.example.com",
            "Dead Node",
        )];
        supervisor
            .sync_endpoints(&mut endpoints, None)
            .await
            .expect("initial sync");
        let endpoint_url = endpoints[0]
            .endpoint_url
            .clone()
            .expect("endpoint url after sync");

        let shared = supervisor
            .shared
            .as_mut()
            .expect("shared process after sync");
        terminate_child_process(&mut shared.child, Duration::from_secs(2))
            .await
            .expect("terminate fake shared xray");

        assert_eq!(
            supervisor
                .acquire_relay_lease_by_url(Some(&endpoint_url))
                .await,
            None
        );
        let snapshot = supervisor.debug_snapshot().await;
        assert_eq!(snapshot.shared_pid, None);
        assert_eq!(snapshot.active_endpoint_handles, 0);
        assert_eq!(snapshot.total_handles, 0);
        assert_eq!(snapshot.retiring_handles, 0);
        assert!(
            snapshot.runtime_files.is_empty(),
            "dead shared process should clear runtime files: {:?}",
            snapshot.runtime_files
        );
    }

    #[tokio::test]
    async fn tavily_proxy_save_and_revalidate_keep_shared_xray_pid() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-flow");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = TavilyProxy::with_options(
            Vec::<String>::new(),
            "http://127.0.0.1:9/mcp",
            db_path
                .to_str()
                .expect("database path should be valid utf-8"),
            TavilyProxyOptions {
                xray_binary: write_fake_xray_binary("proxy-shared-xray-flow"),
                xray_runtime_dir: runtime_dir,
                forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                    .expect("valid trace url"),
                low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
            },
        )
        .await
        .expect("create proxy with fake xray");

        let first_settings = proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![sample_vless_share_link("save-a.example.com", "Save A")],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                true,
            )
            .await
            .expect("save initial settings");
        let first_endpoint_url = first_settings.nodes[0]
            .endpoint_url
            .clone()
            .expect("saved node endpoint url");
        let first_snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        let first_pid = first_snapshot.shared_pid.expect("shared pid after save");

        proxy
            .revalidate_forward_proxy_with_progress(None)
            .await
            .expect("revalidate forward proxy settings");
        let second_snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert_eq!(second_snapshot.shared_pid, Some(first_pid));
        assert_eq!(second_snapshot.active_endpoint_handles, 1);

        let second_settings = proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![sample_vless_share_link("save-b.example.com", "Save B")],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                true,
            )
            .await
            .expect("save updated settings");
        let second_snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert_eq!(second_snapshot.shared_pid, Some(first_pid));
        assert_eq!(second_snapshot.active_endpoint_handles, 1);
        assert_eq!(second_snapshot.total_handles, 1);
        assert_ne!(
            second_settings.nodes[0].endpoint_url.as_deref(),
            Some(first_endpoint_url.as_str())
        );

        proxy.xray_supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn shared_xray_cleanup_failure_keeps_retired_handle_retriable() {
        let runtime_dir = temp_runtime_dir("shared-xray-cleanup-retriable");
        let mut supervisor = XraySupervisor::new(
            write_fake_xray_binary_with_api_failure(
                "shared-xray-cleanup-retriable",
                Some("rmrules"),
            ),
            runtime_dir,
        );

        let mut initial = vec![subscription_vless_endpoint(
            "node-a",
            "a.example.com",
            "Alpha",
        )];
        supervisor
            .sync_endpoints(&mut initial, None)
            .await
            .expect("initial sync");
        let retired_url = initial[0]
            .endpoint_url
            .clone()
            .expect("endpoint url after initial sync");

        let mut changed = vec![subscription_vless_endpoint(
            "node-a",
            "changed.example.com",
            "Alpha New",
        )];
        supervisor
            .sync_endpoints(&mut changed, None)
            .await
            .expect("changed sync");

        let snapshot = supervisor.debug_snapshot().await;
        assert_eq!(snapshot.active_endpoint_handles, 1);
        assert_eq!(snapshot.total_handles, 2);
        assert_eq!(snapshot.retiring_handles, 1);
        assert_eq!(
            supervisor
                .acquire_relay_lease_by_url(Some(&retired_url))
                .await,
            None,
            "failed cleanup should retire the stale relay without exposing it to new selections"
        );

        supervisor.binary = write_fake_xray_binary("shared-xray-cleanup-retriable-recovered");
        supervisor.reap_retired_handles_now().await;
        let recovered_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(recovered_snapshot.active_endpoint_handles, 1);
        assert_eq!(recovered_snapshot.total_handles, 1);
        assert_eq!(recovered_snapshot.retiring_handles, 0);
    }

    #[tokio::test]
    async fn retired_handle_cleanup_does_not_restart_shared_process_after_crash() {
        let runtime_dir = temp_runtime_dir("shared-xray-cleanup-no-restart");
        let mut supervisor = XraySupervisor::new(
            write_fake_xray_binary("shared-xray-cleanup-no-restart"),
            runtime_dir,
        );

        let mut initial = vec![subscription_vless_endpoint(
            "node-a",
            "a.example.com",
            "Alpha",
        )];
        supervisor
            .sync_endpoints(&mut initial, None)
            .await
            .expect("initial sync");
        let old_url = initial[0]
            .endpoint_url
            .clone()
            .expect("old endpoint url after initial sync");
        let lease_id = supervisor
            .acquire_relay_lease_by_url(Some(&old_url))
            .await
            .expect("lease old relay before retirement");

        let mut changed = vec![subscription_vless_endpoint(
            "node-a",
            "changed.example.com",
            "Alpha New",
        )];
        supervisor
            .sync_endpoints(&mut changed, None)
            .await
            .expect("changed sync");
        let pre_crash_snapshot = supervisor.debug_snapshot().await;
        assert!(pre_crash_snapshot.shared_pid.is_some());
        assert_eq!(pre_crash_snapshot.total_handles, 2);
        assert_eq!(pre_crash_snapshot.retiring_handles, 1);

        let shared = supervisor
            .shared
            .as_mut()
            .expect("shared process before crash cleanup");
        terminate_child_process(&mut shared.child, Duration::from_secs(2))
            .await
            .expect("terminate fake shared xray");

        supervisor.release_relay_lease(&lease_id).await;
        let post_crash_snapshot = supervisor.debug_snapshot().await;
        assert_eq!(post_crash_snapshot.shared_pid, None);
        assert_eq!(post_crash_snapshot.active_endpoint_handles, 0);
        assert_eq!(post_crash_snapshot.total_handles, 0);
        assert_eq!(post_crash_snapshot.retiring_handles, 0);
    }

    #[tokio::test]
    async fn send_plan_recovers_after_shared_xray_exit() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-recover");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = TavilyProxy::with_options(
            Vec::<String>::new(),
            "http://127.0.0.1:9/mcp",
            db_path
                .to_str()
                .expect("database path should be valid utf-8"),
            TavilyProxyOptions {
                xray_binary: write_fake_xray_binary("proxy-shared-xray-recover"),
                xray_runtime_dir: runtime_dir,
                forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                    .expect("valid trace url"),
                low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
            },
        )
        .await
        .expect("create proxy with fake xray");

        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![sample_vless_share_link("recover.example.com", "Recover")],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                true,
            )
            .await
            .expect("save proxy-only settings");
        let candidate = proxy
            .build_proxy_attempt_plan_for_record(
                "subject",
                &ForwardProxyAffinityRecord::default(),
                false,
            )
            .await
            .expect("build proxy attempt plan")
            .into_iter()
            .next()
            .expect("proxy-only candidate");

        {
            let mut supervisor = proxy.xray_supervisor.lock().await;
            let shared = supervisor
                .shared
                .as_mut()
                .expect("shared process after settings save");
            terminate_child_process(&mut shared.child, Duration::from_secs(2))
                .await
                .expect("terminate fake shared xray");
        }

        let err = proxy
            .send_with_forward_proxy_plan("subject", None, "request", vec![candidate], |client| {
                client.get("http://127.0.0.1:9/")
            })
            .await
            .expect_err("closed upstream should still fail after relay rebuild");
        assert!(
            matches!(err, ProxyError::Http(_)),
            "request path should recover relay after shared exit instead of surfacing xray_missing: {err}"
        );

        let snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert!(snapshot.shared_pid.is_some());
        assert_eq!(snapshot.active_endpoint_handles, 1);
        assert_eq!(snapshot.total_handles, 1);
        assert_eq!(snapshot.retiring_handles, 0);

        proxy.xray_supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn send_plan_recovers_after_shared_xray_exit_with_held_supervisor_mutex() {
        let root_dir = temp_runtime_dir("proxy-shared-xray-recover-held-lock");
        let db_path = root_dir.join("proxy.db");
        let runtime_dir = root_dir.join("xray-runtime");
        let proxy = Arc::new(
            TavilyProxy::with_options(
                Vec::<String>::new(),
                "http://127.0.0.1:9/mcp",
                db_path
                    .to_str()
                    .expect("database path should be valid utf-8"),
                TavilyProxyOptions {
                    xray_binary: write_fake_xray_binary("proxy-shared-xray-recover-held-lock"),
                    xray_runtime_dir: runtime_dir,
                    forward_proxy_trace_url: Url::parse("http://127.0.0.1/cdn-cgi/trace")
                        .expect("valid trace url"),
                    low_quota_depletion_threshold: LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT,
                },
            )
            .await
            .expect("create proxy with fake xray"),
        );

        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![sample_vless_share_link(
                        "recover-held-lock.example.com",
                        "Recover",
                    )],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                true,
            )
            .await
            .expect("save proxy-only settings");
        let candidate = proxy
            .build_proxy_attempt_plan_for_record(
                "subject",
                &ForwardProxyAffinityRecord::default(),
                false,
            )
            .await
            .expect("build proxy attempt plan")
            .into_iter()
            .next()
            .expect("proxy-only candidate");

        let mut supervisor = proxy.xray_supervisor.lock().await;
        let shared = supervisor
            .shared
            .as_mut()
            .expect("shared process after settings save");
        terminate_child_process(&mut shared.child, Duration::from_secs(2))
            .await
            .expect("terminate fake shared xray");

        let proxy_for_task = Arc::clone(&proxy);
        let send_task = tokio::spawn(async move {
            proxy_for_task
                .send_with_forward_proxy_plan(
                    "subject",
                    None,
                    "request",
                    vec![candidate],
                    |client| client.get("http://127.0.0.1:9/"),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(supervisor);

        let err = send_task
            .await
            .expect("request task should complete")
            .expect_err("closed upstream should still fail after relay rebuild");
        assert!(
            matches!(err, ProxyError::Http(_)),
            "request should retry through a rebuilt relay after shared exit: {err}"
        );

        let snapshot = proxy.xray_supervisor.lock().await.debug_snapshot().await;
        assert!(snapshot.shared_pid.is_some());
        assert_eq!(snapshot.active_endpoint_handles, 1);
        assert_eq!(snapshot.total_handles, 1);
        assert_eq!(snapshot.retiring_handles, 0);

        proxy.xray_supervisor.lock().await.shutdown_all().await;
    }
}
