//! Compiles the real pinned SDK/common and the concrete adapter. Only Tauri is stubbed.
#![allow(dead_code)]
extern crate self as tauri;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tezbar_counter_v2::{Host, HostFuture, StoreChange};

#[derive(Clone)]
pub struct AppHandle;
pub trait Emitter {
    fn emit(&self, channel: &str, payload: Value) -> Result<(), String>;
}
impl Emitter for AppHandle {
    fn emit(&self, _: &str, _: Value) -> Result<(), String> {
        Err("Tauri emission prohibited in fixture".into())
    }
}
#[path = "fixtures/beta/common.rs"]
pub mod common;
pub mod native_core {
    pub use crate::common;
}
#[path = "../tezbar_adapter.rs"]
mod adapter;
#[path = "fixtures/beta/v2_sdk.rs"]
mod v2_sdk;

fn context(root: &std::path::Path) -> common::CoreContext {
    let ctx = common::CoreContext::new(
        common::CorePaths {
            data_dir: root.join("data"),
            config_dir: root.join("config"),
            resource_dir: root.join("resources"),
            temp_dir: root.join("tmp"),
            home_dir: root.into(),
        },
        None,
    );
    ctx.set_router(Arc::new(|channel, _| {
        Box::pin(async move { panic!("Unexpected OS/router request: {channel}") })
    }))
    .unwrap();
    ctx
}
fn path(ctx: &common::CoreContext) -> std::path::PathBuf {
    ctx.paths
        .data_dir
        .join("native-extensions-v2/counter/storage.json")
}
async fn run(ctx: &common::CoreContext, cmd: &str, payload: Value) -> Result<Value, String> {
    let mut state = json!({"unrelated":"preserved"});
    let result = adapter::handle(ctx, cmd, "extension:run-command", &payload, &mut state).await;
    assert_eq!(state, json!({"unrelated":"preserved"}));
    result
}
fn seed(ctx: &common::CoreContext, value: &Value) {
    common::write_json_atomic(&path(ctx), value).unwrap();
}

#[tokio::test]
async fn actual_source_oracle_matches_values_storage_and_metadata_separately() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("oracle.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    for (index, case) in cases.iter().enumerate() {
        let ctx = context(&temp.path().join(index.to_string()));
        seed(&ctx, &case["store"]);
        let before = std::fs::read(path(&ctx)).unwrap();
        let result = run(
            &ctx,
            case["command"].as_str().unwrap(),
            case["payload"].clone(),
        )
        .await;
        if case["expected"]["nativeError"] == true {
            assert!(result.is_err(), "case {index}: {case}");
            assert_eq!(std::fs::read(path(&ctx)).unwrap(), before);
            continue;
        }
        let result = result.unwrap_or_else(|e| panic!("case {index}: {case}: {e}"));
        assert_eq!(
            result["value"].as_f64(),
            case["expected"]["value"].as_f64(),
            "case {index}"
        );
        assert_eq!(result["mode"], "no-view");
        let expected = &case["expected"];
        let actual = common::read_json(&path(&ctx)).unwrap();
        // JSON integer/float representation may differ; compare numeric counter as binary64.
        assert_eq!(
            actual["counter"].as_f64(),
            expected["store"]["counter"].as_f64(),
            "case {index}"
        );
        if expected["writes"] == 0 {
            assert_eq!(std::fs::read(path(&ctx)).unwrap(), before);
        }
        assert_eq!(actual.get("future"), expected["store"].get("future"));
        if case["command"] == "reset-counter" {
            assert_eq!(expected["metadata"], json!([]));
            assert_eq!(result["message"], "Value: 0"); // Explicit native UI adaptation.
        } else {
            let metadata =
                tezbar_counter_v2::command_metadata(result["value"].as_f64().unwrap()).unwrap();
            assert_eq!(metadata, expected["metadata"][0], "case {index}");
            assert_eq!(result["message"], metadata["subtitle"]);
        }
    }
}

#[tokio::test]
async fn read_only_missing_store_restart_and_namespace_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    for no_update in [json!(true), json!("false"), json!([]), json!({}), json!(1)] {
        for cmd in ["increment-counter", "decrement-counter"] {
            let result = run(&ctx, cmd, json!({"launchContext":{"noUpdate":no_update}}))
                .await
                .unwrap();
            assert_eq!(result["message"], "Value: 0");
            assert!(!path(&ctx).exists());
        }
    }
    assert!(!ctx.paths.data_dir.exists());
    v2_sdk::update_store(&ctx, "count-numbers", |v| {
        v["counter"] = json!(99);
        Ok(())
    })
    .await
    .unwrap();
    let legacy = ctx.paths.data_dir.join("extensions/counter/storage.json");
    common::write_json_atomic(&legacy, &json!({"counter":123})).unwrap();
    let original = std::fs::read(&legacy).unwrap();
    assert_eq!(
        run(&ctx, "increment-counter", json!({"noUpdate":true}))
            .await
            .unwrap()["value"],
        1.0
    );
    let restarted = context(temp.path());
    assert_eq!(
        run(&restarted, "decrement-counter", json!({}))
            .await
            .unwrap()["value"],
        0.0
    );
    assert_eq!(
        v2_sdk::read_store(&ctx, "count-numbers").await.unwrap()["counter"],
        99
    );
    assert_eq!(std::fs::read(legacy).unwrap(), original);
    assert!(!ctx.paths.config_dir.exists());
}

#[tokio::test]
async fn corruption_validation_reset_repair_and_callback_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    seed(&ctx, &json!({"counter":9,"unknown":{"keep":true}}));
    let before = std::fs::read(path(&ctx)).unwrap();
    assert!(v2_sdk::update_store(&ctx, "counter", |v| {
        v["counter"] = json!(100);
        Err("reject".into())
    })
    .await
    .is_err());
    assert_eq!(std::fs::read(path(&ctx)).unwrap(), before);
    for raw in ["{broken", "[]", "null", "1"] {
        std::fs::write(path(&ctx), raw).unwrap();
        for cmd in adapter::commands() {
            assert!(run(&ctx, cmd, json!({})).await.is_err());
            assert_eq!(std::fs::read(path(&ctx)).unwrap(), raw.as_bytes());
        }
        assert!(run(
            &ctx,
            "increment-counter",
            json!({"launchContext":{"noUpdate":true}})
        )
        .await
        .is_err());
    }
    for bad in [
        json!("NaN"),
        json!(true),
        json!({}),
        json!([]),
        json!("9".repeat(400)),
    ] {
        seed(&ctx, &json!({"counter":bad,"unknown":{"keep":true}}));
        let before = std::fs::read(path(&ctx)).unwrap();
        for cmd in ["increment-counter", "decrement-counter"] {
            assert!(run(&ctx, cmd, json!({})).await.is_err());
            assert_eq!(std::fs::read(path(&ctx)).unwrap(), before);
        }
        assert_eq!(
            run(
                &ctx,
                "reset-counter",
                json!({"launchContext":{"noUpdate":true}})
            )
            .await
            .unwrap()["message"],
            "Value: 0"
        );
        assert_eq!(
            common::read_json(&path(&ctx)).unwrap(),
            json!({"counter":0.0,"unknown":{"keep":true}})
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_commands_across_independent_contexts_do_not_lose_updates() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    seed(&ctx, &json!({"counter":0,"future": [1,2]}));
    let mut tasks = Vec::new();
    for index in 0..120 {
        let ctx = context(temp.path());
        tasks.push(tokio::spawn(async move {
            run(
                &ctx,
                if index % 3 == 0 {
                    "decrement-counter"
                } else {
                    "increment-counter"
                },
                json!({}),
            )
            .await
            .unwrap()
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(
        common::read_json(&path(&ctx)).unwrap(),
        json!({"counter":40.0,"future":[1,2]})
    );
}

#[derive(Debug)]
struct Call {
    start: usize,
    end: usize,
    delta: Option<i32>,
    result: i32,
}
fn linearizable(calls: &[Call], mask: usize, value: i32, final_value: i32) -> bool {
    if mask == (1 << calls.len()) - 1 {
        return value == final_value;
    }
    calls.iter().enumerate().any(|(i, c)| {
        let next = c.delta.map_or(0, |d| value + d);
        mask & (1 << i) == 0
            && next == c.result
            && !calls
                .iter()
                .enumerate()
                .any(|(j, other)| mask & (1 << j) == 0 && other.end < c.start)
            && linearizable(calls, mask | (1 << i), next, final_value)
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mixed_reset_histories_are_linearizable_including_real_time_order() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    for _ in 0..12 {
        run(&ctx, "reset-counter", json!({})).await.unwrap();
        let clock = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for delta in [
            Some(1),
            Some(1),
            None,
            Some(-1),
            Some(1),
            None,
            Some(-1),
            Some(1),
        ] {
            let ctx = context(temp.path());
            let clock = clock.clone();
            tasks.push(tokio::spawn(async move {
                let start = clock.fetch_add(1, Ordering::SeqCst);
                let command = match delta {
                    Some(1) => "increment-counter",
                    Some(_) => "decrement-counter",
                    None => "reset-counter",
                };
                let result = run(&ctx, command, json!({})).await.unwrap()["value"]
                    .as_f64()
                    .unwrap() as i32;
                let end = clock.fetch_add(1, Ordering::SeqCst);
                Call {
                    start,
                    end,
                    delta,
                    result,
                }
            }));
            tokio::task::yield_now().await;
        }
        let mut calls = Vec::new();
        for task in tasks {
            calls.push(task.await.unwrap());
        }
        let final_value = common::read_json(&path(&ctx)).unwrap()["counter"]
            .as_f64()
            .unwrap() as i32;
        assert!(
            linearizable(&calls, 0, 0, final_value),
            "{calls:?}; final {final_value}"
        );
    }
}

struct FailureHost {
    reply: Value,
    error: bool,
    reads: AtomicUsize,
    updates: AtomicUsize,
}
impl Host for FailureHost {
    fn read_store(&self) -> HostFuture<'_> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.reply()
    }
    fn update_store(&self, _: StoreChange) -> HostFuture<'_> {
        self.updates.fetch_add(1, Ordering::SeqCst);
        self.reply()
    }
}
impl FailureHost {
    fn reply(&self) -> HostFuture<'_> {
        Box::pin(async {
            if self.error {
                Err("HostErr".into())
            } else {
                Ok(self.reply.clone())
            }
        })
    }
}
#[tokio::test]
async fn host_failures_never_produce_success_hud_and_operations_are_bounded() {
    for reply in [
        json!({"ok":false,"error":"specific error"}),
        json!({"ok":false,"message":"specific message"}),
        json!([]),
    ] {
        for error in [false, true] {
            let host = FailureHost {
                reply: reply.clone(),
                error,
                reads: AtomicUsize::new(0),
                updates: AtomicUsize::new(0),
            };
            for no_update in [false, true] {
                let result = tezbar_counter_v2::handle(
                    &host,
                    "increment-counter",
                    "extension:run-command",
                    &json!({"launchContext":{"noUpdate":no_update}}),
                    &mut Value::Null,
                )
                .await;
                let message = result.unwrap_err();
                if error {
                    assert_eq!(message, "HostErr");
                } else if let Some(expected) = reply["error"].as_str().or(reply["message"].as_str())
                {
                    assert_eq!(message, expected);
                }
            }
            assert_eq!(host.reads.load(Ordering::SeqCst), 1);
            assert_eq!(host.updates.load(Ordering::SeqCst), 1);
            let before = host.updates.load(Ordering::SeqCst);
            for (cmd, op) in [
                ("increment-counter", "extension:search"),
                ("other", "extension:run-command"),
                ("other", "extension:dispose-session"),
            ] {
                assert!(
                    tezbar_counter_v2::handle(&host, cmd, op, &json!({}), &mut Value::Null)
                        .await
                        .is_err()
                );
            }
            assert_eq!(
                tezbar_counter_v2::handle(
                    &host,
                    "increment-counter",
                    "extension:dispose-session",
                    &json!({}),
                    &mut Value::Null
                )
                .await
                .unwrap()["mode"],
                "unchanged"
            );
            assert_eq!(host.updates.load(Ordering::SeqCst), before);
        }
    }
    assert!(tezbar_counter_v2::command_metadata(f64::NAN).is_err());
    assert!(tezbar_counter_v2::command_metadata(f64::INFINITY).is_err());
}

#[test]
fn manifest_source_metadata_icon_and_schema_contract() {
    use sha2::{Digest, Sha256};
    let manifest: Value = serde_json::from_str(adapter::MANIFEST).unwrap();
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/beta/extension-v2.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&manifest)
        .unwrap();
    let source: Value = serde_json::from_str(include_str!("fixtures/v1/package.json")).unwrap();
    assert_eq!(manifest["id"], "com.tezbar.native.counter");
    assert_eq!(manifest["replaces"], json!(["raycast.counter"]));
    assert_eq!(manifest["icon"], "/extensions/icons/counter.png");
    assert_eq!(manifest["preferences"], json!([]));
    assert_eq!(manifest["platforms"], source["platforms"]);
    for (native, original) in manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .zip(source["commands"].as_array().unwrap())
    {
        for key in [
            "name",
            "title",
            "description",
            "mode",
            "subtitle",
            "platforms",
        ] {
            assert_eq!(native.get(key), original.get(key));
        }
        assert_eq!(native["runtime"], "rust");
    }
    assert_eq!(
        manifest["commands"].as_array().unwrap().len(),
        adapter::commands().len()
    );
    for (command, name) in manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .zip(adapter::commands())
    {
        assert_eq!(command["name"], *name);
    }
    for qa in manifest["verification"].as_array().unwrap() {
        assert_eq!(qa["state"], "not-run");
    }
    assert_eq!(adapter::ICON.len(), 23168);
    assert_eq!(
        format!("{:x}", Sha256::digest(adapter::ICON)),
        "06961936531a678d738f83befba4ff04cfe43c9b53de8aca6d758aa229c9e3be"
    );
}

#[tokio::test]
async fn repeated_commands_reparse_persisted_binary64_through_js_number_string() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    seed(&ctx, &json!({"counter":"1000000000000000000000"}));
    assert_eq!(
        run(&ctx, "increment-counter", json!({})).await.unwrap()["message"],
        "Value: 1e+21"
    );
    assert_eq!(
        run(&ctx, "increment-counter", json!({})).await.unwrap()["message"],
        "Value: 2"
    );
    assert_eq!(
        run(&ctx, "decrement-counter", json!({})).await.unwrap()["message"],
        "Value: 1"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn actual_sdk_write_failure_preserves_committed_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    seed(&ctx, &json!({"counter":7,"unknown":42}));
    let file = path(&ctx);
    let directory = file.parent().unwrap();
    let before = std::fs::read(&file).unwrap();
    let permissions = std::fs::metadata(directory).unwrap().permissions();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = run(&ctx, "increment-counter", json!({})).await;
    std::fs::set_permissions(directory, permissions).unwrap();
    assert!(
        result.is_err(),
        "write-failure test requires a user without permission override"
    );
    assert_eq!(std::fs::read(file).unwrap(), before);
}

#[tokio::test]
async fn namespace_keys_cannot_collide_with_host_failure_envelope() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let data = json!({"ok":false,"error":"stored data","message":"keep","store":{"counter":99}});
    seed(&ctx, &data);
    assert_eq!(
        run(
            &ctx,
            "increment-counter",
            json!({"launchContext":{"noUpdate":true}})
        )
        .await
        .unwrap()["value"],
        0.0
    );
    assert_eq!(common::read_json(&path(&ctx)).unwrap(), data);
    for command in adapter::commands() {
        run(&ctx, command, json!({})).await.unwrap();
        let mut stored = common::read_json(&path(&ctx)).unwrap();
        stored.as_object_mut().unwrap().remove("counter");
        assert_eq!(stored, data);
    }
}
