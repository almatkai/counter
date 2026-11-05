//! Drives the real executable over JSONL pipes with a fake, narrow host transport.
//! Counter declares no capabilities: any reverse `host.*` request fails the test.
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

const BIN: &str = env!("CARGO_BIN_EXE_tezbar-counter");
const ID: &str = "com.tezbar.native.counter";

fn manifest() -> Value {
    serde_json::from_str(include_str!("../extension.json")).unwrap()
}

struct Peer {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    reverse: Vec<Value>,
    seq: u64,
}

impl Peer {
    async fn start() -> Self {
        let mut child = Command::new(BIN)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env_clear()
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            reverse: vec![],
            seq: 0,
        }
    }
    async fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        let input = self.input.as_mut().unwrap();
        input.write_all(&bytes).await.unwrap();
        input.flush().await.unwrap();
    }
    async fn frame(&mut self) -> Value {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), self.output.read_line(&mut line))
            .await
            .expect("protocol timeout")
            .unwrap();
        assert!(n > 0, "unexpected EOF");
        assert!(line.ends_with('\n'), "frames are LF terminated");
        serde_json::from_str(&line).unwrap()
    }
    fn next_id(&mut self) -> String {
        self.seq += 1;
        format!("h:00000000-0000-4000-8000-{:012x}", self.seq)
    }
    /// Sends a host request and returns Ok(result) or Err(error frame).
    async fn submit(&mut self, method: &str, params: Value) -> String {
        let id = self.next_id();
        self.send(json!({"type":"request","id":id,"method":method,"params":params}))
            .await;
        id
    }
    async fn collect(&mut self, mut ids: Vec<String>) -> HashMap<String, Result<Value, Value>> {
        let mut out = HashMap::new();
        while !ids.is_empty() {
            let frame = self.frame().await;
            match frame["type"].as_str().unwrap() {
                "response" => {
                    let id = frame["id"].as_str().unwrap().to_owned();
                    if let Some(i) = ids.iter().position(|x| *x == id) {
                        ids.remove(i);
                        let r = match frame.get("error") {
                            Some(e) => {
                                assert_eq!(e["code"], "extension_error");
                                assert!(e["message"].as_str().is_some_and(|s| !s.is_empty()));
                                Err(e.clone())
                            }
                            None => Ok(frame["result"].clone()),
                        };
                        out.insert(id, r);
                    }
                }
                "request" => {
                    // Narrow fake host: nothing is granted, so every reverse RPC is refused.
                    self.reverse.push(frame.clone());
                    self.send(json!({"type":"response","id":frame["id"],"error":{"code":"capability_denied","message":"capability not granted"}})).await;
                }
                "event" => {}
                other => panic!("unexpected frame type {other}: {frame}"),
            }
        }
        out
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        let id = self.submit(method, params).await;
        self.collect(vec![id.clone()]).await.remove(&id).unwrap()
    }
    async fn initialize(&mut self, dir: &Path, migrations: Value) {
        let hello = self.frame().await;
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["id"], ID);
        assert_eq!(hello["protocol"], 1);
        assert_eq!(hello["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(hello["sdkVersion"], "0.1.1");
        assert!(hello["features"]
            .as_array()
            .unwrap()
            .contains(&json!("launcher.v1")));
        let r = self
            .call(
                "initialize",
                json!({"protocol":1,"extensionId":ID,"platform":host_platform(),
                "packageDir":env!("CARGO_MANIFEST_DIR"),"dataDir":dir.join("data"),"tempDir":dir.join("temp"),
                "grantedCapabilities":[],"reason":"command","migrationPaths":migrations,"hostFeatures":["launcher.v1"]}),
            )
            .await
            .unwrap();
        assert_eq!(r["ready"], true);
        assert!(r["features"]
            .as_array()
            .unwrap()
            .contains(&json!("launcher.v1")));
    }
    async fn run(&mut self, session: &str, command: &str, extra: Value) -> Result<Value, Value> {
        let mut params =
            json!({"sessionId":session,"commandName":command,"preferences":{},"argumentValues":{}});
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        let result = self.call("session.open", params).await;
        if let Ok(value) = &result {
            launcher_message(value);
            self.call("session.dispose", json!({"sessionId":session}))
                .await
                .unwrap();
        }
        result
    }
    async fn shutdown(mut self) {
        self.call("shutdown", json!({})).await.unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert!(
            self.reverse.is_empty(),
            "unexpected host RPC: {:?}",
            self.reverse
        );
    }
}

// process_install::target() and native.rs session annotation leave successful
// no-view results intact. CommandBar.executeExtensionCommandViaRuntime checks
// `ok`, skips the view branch, and passes `message` to showActionMsg. Checking
// effects alone would not establish visible initial command feedback.
fn launcher_message(result: &Value) -> &str {
    assert_eq!(result["ok"], true);
    assert_eq!(result["mode"], "no-view");
    let message = result["message"]
        .as_str()
        .expect("launcher message required");
    assert!(!message.is_empty());
    message
}

fn host_platform() -> String {
    format!(
        "{}-{}",
        std::env::consts::ARCH,
        if cfg!(windows) {
            "pc-windows-msvc"
        } else {
            "apple-darwin"
        }
    )
}

// Match process_host::migration_paths: only declared existing source files are
// forwarded from the profile. Fixtures contain no symlinks or directories.
fn migration_grants(profile: &Path) -> Value {
    manifest()["migrationPaths"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, relative)| {
            let path = profile.join(relative.as_str().unwrap());
            path.is_file().then(|| (key.clone(), json!(path)))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn store(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("data/counter/storage.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn protocol_info_matches_manifest_and_original_commands() {
    let out = Command::new(BIN)
        .arg("--protocol-info")
        .output()
        .await
        .unwrap();
    assert!(out.status.success());
    let info: Value = serde_json::from_slice(&out.stdout).unwrap();
    let m = manifest();
    assert_eq!(info["protocol"], 1);
    assert_eq!(info["id"], m["id"]);
    assert_eq!(info["version"], m["version"]);
    let names: Vec<&str> = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(info["commands"], json!(names));
}

#[test]
fn manifest_is_schema_3_and_preserves_original_package_metadata() {
    let m = manifest();
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/extension-v3.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator.iter_errors(&m).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let pkg: Value = serde_json::from_str(include_str!("../package.json")).unwrap();
    assert_eq!(
        m["id"],
        format!("com.tezbar.native.{}", m["slug"].as_str().unwrap())
    );
    assert_eq!(m["slug"], pkg["name"]);
    assert_eq!(
        m["replaces"],
        json!([format!("raycast.{}", pkg["name"].as_str().unwrap())])
    );
    assert_eq!(m["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["capabilities"], json!([]));
    assert_eq!(m["platforms"], pkg["platforms"]);
    let original = pkg["commands"].as_array().unwrap();
    let ported = m["commands"].as_array().unwrap();
    assert_eq!(original.len(), ported.len());
    for (o, p) in original.iter().zip(ported) {
        for key in [
            "name",
            "title",
            "description",
            "mode",
            "subtitle",
            "platforms",
        ] {
            assert_eq!(o.get(key), p.get(key), "{key}");
        }
        assert_eq!(p["runtime"], "rust");
    }
    assert_eq!(pkg["preferences"], Value::Null);
    assert_eq!(m["preferences"], json!([]));
    assert!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(m["icon"].as_str().unwrap())
        .is_file());
    assert_eq!(
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(m["icon"].as_str().unwrap()))
            .unwrap(),
        include_bytes!("../command-icon.png")
    );
}

#[tokio::test]
async fn migrated_counter_increment_decrement_no_update_reset_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy-local-storage.json");
    let legacy_bytes = br#"{"counter":"41"}"#;
    std::fs::write(&legacy, legacy_bytes).unwrap();
    let mut peer = Peer::start().await;
    peer.initialize(
        dir.path(),
        json!({"legacyLocalStorage": legacy, "nativeStore": dir.path().join("absent.json")}),
    )
    .await;
    assert_eq!(
        store(dir.path())["_tezbarMigration"]["source"],
        "legacyLocalStorage"
    );

    let r = peer
        .run("s1", "increment-counter", json!({}))
        .await
        .unwrap();
    assert_eq!(r["mode"], "no-view");
    assert_eq!(r["message"], "Value: 42");
    assert_eq!(store(dir.path())["counter"].as_f64(), Some(42.0));

    let before = std::fs::read(dir.path().join("data/counter/storage.json")).unwrap();
    let r = peer
        .run(
            "s2",
            "decrement-counter",
            json!({"launchContext":{"noUpdate":true}}),
        )
        .await
        .unwrap();
    assert_eq!(r["message"], "Value: 42");
    assert_eq!(
        std::fs::read(dir.path().join("data/counter/storage.json")).unwrap(),
        before
    );

    assert_eq!(
        peer.run("s3", "decrement-counter", json!({}))
            .await
            .unwrap()["message"],
        "Value: 41"
    );
    assert_eq!(
        peer.run("s4", "decrement-counter", json!({}))
            .await
            .unwrap()["value"],
        40.0
    );

    // Reset ignores launch context, as the original source does.
    let r = peer
        .run(
            "s5",
            "reset-counter",
            json!({"launchContext":{"noUpdate":true}}),
        )
        .await
        .unwrap();
    assert_eq!(r["message"], "Value: 0");
    assert_eq!(store(dir.path())["counter"].as_f64(), Some(0.0));
    assert_eq!(
        peer.run("s6", "decrement-counter", json!({}))
            .await
            .unwrap()["message"],
        "Value: -1"
    );

    // No actions exist; unbound operations and unknown commands are rejected.
    peer.call("session.open", json!({"sessionId":"s7","commandName":"increment-counter","preferences":{},"argumentValues":{}})).await.unwrap();
    assert!(peer
        .call(
            "session.call",
            json!({"sessionId":"s7","operation":"extension:invoke-action","payload":{"actionId":"x"}})
        )
        .await
        .is_err());
    peer.call("session.dispose", json!({"sessionId":"s7"}))
        .await
        .unwrap();
    assert_eq!(
        store(dir.path())["counter"].as_f64(),
        Some(0.0),
        "unbound action had no effect"
    );
    assert!(peer.run("s8", "count-numbers", json!({})).await.is_err());

    // Malformed private store fails without being replaced, including on reset.
    std::fs::write(dir.path().join("data/counter/storage.json"), b"{broken").unwrap();
    assert!(peer
        .run("s9", "increment-counter", json!({}))
        .await
        .is_err());
    assert!(peer.run("s10", "reset-counter", json!({})).await.is_err());
    assert_eq!(
        std::fs::read(dir.path().join("data/counter/storage.json")).unwrap(),
        b"{broken"
    );

    assert_eq!(
        std::fs::read(&legacy).unwrap(),
        legacy_bytes,
        "legacy store is never modified"
    );
    peer.shutdown().await;
}

#[tokio::test]
async fn each_host_granted_migration_layout_and_resolver_precedence_survives_restart() {
    // V2 first, followed by extension-registry.ts resolver candidates in order.
    let ordered = [
        ("nativeStore", "native-extensions-v2/counter/storage.json"),
        (
            "legacyLocalStorage",
            "extensions/counter/.tezbar-local-storage.json",
        ),
        (
            "legacyRaycastStorage",
            "extensions/raycast.counter/.tezbar-local-storage.json",
        ),
        (
            "legacyPackageStorage",
            "extensions/packages/raycast.counter/.tezbar-local-storage.json",
        ),
        (
            "legacyRegistryStorage",
            "extension-registry/packages/raycast.counter/.tezbar-local-storage.json",
        ),
    ];
    // Each layout alone plus all nonempty suffixes proves precedence and fallback.
    for selected in (0..ordered.len())
        .map(|i| vec![i])
        .chain((0..ordered.len() - 1).map(|i| (i..ordered.len()).collect()))
    {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        let mut originals = vec![];
        for &i in &selected {
            let (key, relative) = ordered[i];
            assert_eq!(manifest()["migrationPaths"][key], relative);
            let path = profile.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = serde_json::to_vec(
                &json!({"counter": format!("{} items", 41 + i), "legacyOnly": "keep"}),
            )
            .unwrap();
            std::fs::write(&path, &bytes).unwrap();
            originals.push((path, bytes));
        }
        let grants = migration_grants(&profile);
        assert_eq!(grants.as_object().unwrap().len(), selected.len());
        let expected = (42 + selected[0]) as f64;
        let mut peer = Peer::start().await;
        peer.initialize(dir.path(), grants.clone()).await;
        assert_eq!(
            store(dir.path())["_tezbarMigration"]["source"],
            ordered[selected[0]].0
        );
        let result = peer
            .run("migrated", "increment-counter", json!({}))
            .await
            .unwrap();
        assert_eq!(launcher_message(&result), format!("Value: {expected}"));
        assert_eq!(store(dir.path())["counter"].as_f64(), Some(expected));
        assert_eq!(
            store(dir.path()).get("legacyOnly").is_some(),
            selected[0] == 0
        );
        peer.shutdown().await;

        let before = std::fs::read(dir.path().join("data/counter/storage.json")).unwrap();
        let mut again = Peer::start().await;
        again.initialize(dir.path(), grants).await;
        let result = again
            .run(
                "restart",
                "decrement-counter",
                json!({"launchContext":{"noUpdate":true}}),
            )
            .await
            .unwrap();
        assert_eq!(launcher_message(&result), format!("Value: {expected}"));
        assert_eq!(
            std::fs::read(dir.path().join("data/counter/storage.json")).unwrap(),
            before
        );
        again.shutdown().await;
        for (path, bytes) in originals {
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "legacy bytes survive initialization, commands and restart"
            );
        }
    }
}

#[tokio::test]
async fn intentional_private_counters_and_completed_empty_source_markers_are_never_recovered() {
    for counter in [0, 7, -3] {
        for marked in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let profile = dir.path().join("profile");
            let source = profile.join("extensions/raycast.counter/.tezbar-local-storage.json");
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            let legacy = br#"{"counter":"41"}"#;
            std::fs::write(&source, legacy).unwrap();
            let mut private = json!({"counter":counter,"future":{"keep":true}});
            if marked {
                private["_tezbarMigration"] = json!({"version":1,"source":null});
            }
            let path = dir.path().join("data/counter/storage.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let before = serde_json::to_vec(&private).unwrap();
            std::fs::write(&path, &before).unwrap();
            let mut peer = Peer::start().await;
            peer.initialize(dir.path(), migration_grants(&profile))
                .await;
            let result = peer
                .run(
                    "private",
                    "increment-counter",
                    json!({"launchContext":{"noUpdate":true}}),
                )
                .await
                .unwrap();
            assert_eq!(launcher_message(&result), format!("Value: {counter}"));
            assert_eq!(store(dir.path())["counter"], counter);
            assert_eq!(store(dir.path())["future"], private["future"]);
            assert_eq!(store(dir.path())["_tezbarMigration"]["source"], Value::Null);
            if marked {
                assert_eq!(std::fs::read(path).unwrap(), before);
            }
            assert_eq!(std::fs::read(source).unwrap(), legacy);
            peer.shutdown().await;
        }
    }
}

#[tokio::test]
async fn pipelined_sessions_are_transactional_and_value_persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = Peer::start().await;
    peer.initialize(dir.path(), json!({})).await;
    let mut ids = vec![];
    for i in 0..24 {
        let command = if i % 3 == 2 {
            "decrement-counter"
        } else {
            "increment-counter"
        };
        ids.push(peer.submit("session.open", json!({"sessionId":format!("p{i}"),"commandName":command,"preferences":{},"argumentValues":{}})).await);
    }
    let results = peer.collect(ids.clone()).await;
    let mut values: Vec<f64> = ids
        .iter()
        .map(|id| results[id].as_ref().unwrap()["value"].as_f64().unwrap())
        .collect();
    assert_eq!(store(dir.path())["counter"].as_f64(), Some(8.0));
    values.sort_by(f64::total_cmp);
    assert!(values.iter().all(|v| (-1.0..=16.0).contains(v)));
    for i in 0..24 {
        peer.call("session.dispose", json!({"sessionId":format!("p{i}")}))
            .await
            .unwrap();
    }
    peer.shutdown().await;

    let mut again = Peer::start().await;
    again
        .initialize(
            dir.path(),
            json!({"nativeStore": dir.path().join("ignored.json")}),
        )
        .await;
    let r = again
        .run(
            "restart",
            "increment-counter",
            json!({"launchContext":{"noUpdate":"false"}}),
        )
        .await
        .unwrap();
    assert_eq!(
        r["message"], "Value: 8",
        "JS-truthy noUpdate string reads only"
    );
    assert_eq!(
        again
            .run("restart2", "increment-counter", json!({}))
            .await
            .unwrap()["message"],
        "Value: 9"
    );
    again.shutdown().await;
}

#[tokio::test]
async fn eof_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = Peer::start().await;
    peer.initialize(dir.path(), json!({})).await;
    peer.input.take();
    let status = tokio::time::timeout(Duration::from_secs(5), peer.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
}
