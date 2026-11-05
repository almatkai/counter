//! Maps the reused Native V2 `Host` trait onto SDK-scoped private storage.
//!
//! Counter needs no reverse `host.*` RPC: SDK storage is local to the granted
//! `dataDir`, serialized by the SDK transaction lock and written atomically.
use serde_json::json;
use tezbar_counter_v2::{Host, HostFuture, StoreChange};
use tezbar_extension_sdk::{read_store, update_store, ExtensionContext};

/// Private store name: `<dataDir>/counter/storage.json`.
pub const STORE: &str = "counter";

pub struct SdkHost<'a>(pub &'a ExtensionContext);

impl Host for SdkHost<'_> {
    fn read_store(&self) -> HostFuture<'_> {
        Box::pin(async move {
            let store = read_store(self.0, STORE).await?;
            Ok(json!({"ok": true, "store": store}))
        })
    }
    fn update_store(&self, change: StoreChange) -> HostFuture<'_> {
        Box::pin(async move {
            let store = update_store(self.0, STORE, change).await?;
            Ok(json!({"ok": true, "store": store}))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tezbar_extension_sdk::{ExtensionPaths, TestContext};

    fn fixture(dir: &std::path::Path) -> TestContext {
        TestContext::new(
            ExtensionPaths {
                package_dir: dir.join("package"),
                data_dir: dir.join("data"),
                temp_dir: dir.join("tmp"),
            },
            &[],
        )
    }
    async fn run(ctx: &ExtensionContext, command: &str, payload: Value) -> Result<Value, String> {
        let mut state = json!({});
        let result = tezbar_counter_v2::handle(
            &SdkHost(ctx),
            command,
            "extension:run-command",
            &payload,
            &mut state,
        )
        .await;
        assert_eq!(state, json!({}));
        result
    }

    #[tokio::test]
    async fn increments_decrements_resets_with_private_store_and_no_rpc() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        let ctx = &f.context;
        assert_eq!(
            run(ctx, "increment-counter", json!({})).await.unwrap()["message"],
            "Value: 1"
        );
        assert_eq!(
            run(ctx, "increment-counter", json!({})).await.unwrap()["message"],
            "Value: 2"
        );
        assert_eq!(
            run(ctx, "decrement-counter", json!({})).await.unwrap()["value"],
            1.0
        );
        let stored = read_store(ctx, STORE).await.unwrap();
        assert_eq!(stored["counter"].as_f64(), Some(1.0));
        assert!(dir.path().join("data/counter/storage.json").is_file());
        assert_eq!(
            run(
                ctx,
                "reset-counter",
                json!({"launchContext":{"noUpdate":true}})
            )
            .await
            .unwrap()["message"],
            "Value: 0"
        );
        assert!(f.calls().is_empty(), "Counter must not issue host RPC");
    }

    #[tokio::test]
    async fn no_update_is_read_only_and_does_not_create_store() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        let r = run(
            &f.context,
            "decrement-counter",
            json!({"launchContext":{"noUpdate":true}}),
        )
        .await
        .unwrap();
        assert_eq!(r["message"], "Value: 0");
        assert!(!dir.path().join("data").exists());
    }

    #[tokio::test]
    async fn invalid_and_malformed_stores_fail_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        let path = dir.path().join("data/counter/storage.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"counter":true,"keep":1}"#).unwrap();
        assert!(run(&f.context, "increment-counter", json!({}))
            .await
            .is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            br#"{"counter":true,"keep":1}"#
        );
        // Reset repairs an invalid counter but keeps unknown keys.
        run(&f.context, "reset-counter", json!({})).await.unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["counter"].as_f64(), Some(0.0));
        assert_eq!(v["keep"], 1);
        std::fs::write(&path, b"{broken").unwrap();
        assert!(run(&f.context, "reset-counter", json!({})).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    }

    #[tokio::test]
    async fn javascript_parse_int_semantics_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        let path = dir.path().join("data/counter/storage.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"counter":" 0x1Fz"}"#).unwrap();
        assert_eq!(
            run(&f.context, "increment-counter", json!({}))
                .await
                .unwrap()["value"],
            32.0
        );
        std::fs::write(&path, br#"{"counter":"-7.9 items"}"#).unwrap();
        assert_eq!(
            run(&f.context, "decrement-counter", json!({}))
                .await
                .unwrap()["value"],
            -8.0
        );
    }

    #[tokio::test]
    async fn unknown_command_and_operation_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        assert!(run(&f.context, "count", json!({})).await.is_err());
        let mut state = json!({});
        assert!(tezbar_counter_v2::handle(
            &SdkHost(&f.context),
            "increment-counter",
            "extension:action",
            &json!({}),
            &mut state
        )
        .await
        .is_err());
        assert!(!dir.path().join("data").exists());
    }
}
