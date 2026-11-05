//! One-time, explicit import of the counter value from host-granted legacy stores.
//!
//! Sources are only the `migrationPaths` declared in `extension.json` and granted by
//! the host at initialize. Legacy files are read, never modified or deleted.
//! Priority: an existing private store value, then the Native V2 store, then the
//! JavaScript compatibility runtime's LocalStorage shim files in declared order.
use crate::host::STORE;
use serde_json::{json, Map, Value};
use tezbar_extension_sdk::{read_store, update_store, CoreResult, ExtensionContext};

/// Marker key written into the private store once import has been decided.
pub const MARKER: &str = "_tezbarMigration";
/// Native V2 store `{counter: <json>, ...}`; unknown keys are preserved too.
pub const NATIVE_STORE: &str = "nativeStore";
/// Raycast `LocalStorage` shim files (`Record<string,string>`), in priority order.
pub const LOCAL_STORAGE: [&str; 3] = [
    "legacyLocalStorage",
    "legacyPackageStorage",
    "legacyRegistryStorage",
];

enum Source {
    Missing,
    Invalid(String),
    Found(Map<String, Value>),
}

async fn load(ctx: &ExtensionContext, key: &str) -> Source {
    let Some(path) = ctx.migration_paths.get(key) else {
        return Source::Missing;
    };
    match tokio::fs::read(path).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Source::Missing,
        Err(e) => Source::Invalid(format!("{key}: {e}")),
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => Source::Found(map),
            Ok(_) => Source::Invalid(format!("{key}: store is not a JSON object")),
            Err(e) => Source::Invalid(format!("{key}: {e}")),
        },
    }
}

/// Returns the private store after migration. Malformed legacy sources are never
/// imported and leave the marker unset so a later start retries; the extension
/// itself stays usable with the private store.
pub async fn initialize(ctx: &ExtensionContext) -> CoreResult {
    let current = read_store(ctx, STORE).await?;
    if current.get(MARKER).is_some() {
        return Ok(current);
    }
    let mut invalid = Vec::new();
    let mut import: Option<(&str, Map<String, Value>)> = None;
    if current.get("counter").is_none() {
        match load(ctx, NATIVE_STORE).await {
            Source::Found(map) if map.contains_key("counter") => import = Some((NATIVE_STORE, map)),
            Source::Invalid(e) => invalid.push(e),
            _ => {}
        }
        if import.is_none() {
            for key in LOCAL_STORAGE {
                match load(ctx, key).await {
                    Source::Found(map) => {
                        if let Some(value) = map.get("counter") {
                            // The shim stores String(value); keep its exact text so
                            // the reused engine applies JavaScript parseInt to it.
                            let mut only = Map::new();
                            only.insert("counter".into(), value.clone());
                            import = Some((key, only));
                            break;
                        }
                    }
                    Source::Invalid(e) => invalid.push(e),
                    Source::Missing => {}
                }
            }
        }
    }
    if import.is_none() && !invalid.is_empty() {
        eprintln!("counter: legacy store not imported: {}", invalid.join("; "));
        return Ok(current);
    }
    let source = import.as_ref().map(|(key, _)| key.to_string());
    update_store(ctx, STORE, move |store| {
        let target = store.as_object_mut().ok_or("Store must be object")?;
        if let Some((_, values)) = import {
            for (key, value) in values {
                if key != MARKER {
                    target.entry(key).or_insert(value);
                }
            }
        }
        target.insert(MARKER.into(), json!({"version": 1, "source": source}));
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tezbar_extension_sdk::{ExtensionPaths, TestContext};

    fn fixture(dir: &std::path::Path, paths: &[(&str, &str)]) -> TestContext {
        let mut f = TestContext::new(
            ExtensionPaths {
                package_dir: dir.join("package"),
                data_dir: dir.join("data"),
                temp_dir: dir.join("tmp"),
            },
            &[],
        );
        for (key, name) in paths {
            f.context
                .migration_paths
                .insert((*key).into(), dir.join(name));
        }
        f
    }

    #[tokio::test]
    async fn native_store_wins_and_preserves_unknown_keys_without_touching_source() {
        let dir = tempfile::tempdir().unwrap();
        let native = br#"{"counter":7,"future":{"keep":true}}"#;
        std::fs::write(dir.path().join("native.json"), native).unwrap();
        std::fs::write(dir.path().join("ls.json"), br#"{"counter":"99"}"#).unwrap();
        let f = fixture(
            dir.path(),
            &[
                (NATIVE_STORE, "native.json"),
                ("legacyLocalStorage", "ls.json"),
            ],
        );
        let store = initialize(&f.context).await.unwrap();
        assert_eq!(store["counter"], 7);
        assert_eq!(store["future"]["keep"], true);
        assert_eq!(store[MARKER]["source"], NATIVE_STORE);
        assert_eq!(
            std::fs::read(dir.path().join("native.json")).unwrap(),
            native
        );
        // Idempotent: a later legacy change is not re-imported.
        std::fs::write(dir.path().join("native.json"), br#"{"counter":1000}"#).unwrap();
        assert_eq!(initialize(&f.context).await.unwrap(), store);
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn local_storage_string_is_imported_in_declared_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), br#"{"other":"x"}"#).unwrap();
        std::fs::write(
            dir.path().join("b.json"),
            br#"{"counter":"41","other":"y"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("c.json"), br#"{"counter":"5"}"#).unwrap();
        let f = fixture(
            dir.path(),
            &[
                ("legacyLocalStorage", "a.json"),
                ("legacyPackageStorage", "b.json"),
                ("legacyRegistryStorage", "c.json"),
            ],
        );
        let store = initialize(&f.context).await.unwrap();
        assert_eq!(store["counter"], "41");
        assert!(
            store.get("other").is_none(),
            "only the counter key is imported from LocalStorage"
        );
        assert_eq!(store[MARKER]["source"], "legacyPackageStorage");
    }

    #[tokio::test]
    async fn existing_private_value_is_kept_and_absent_sources_mark_done() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ls.json"), br#"{"counter":"41"}"#).unwrap();
        let f = fixture(dir.path(), &[("legacyLocalStorage", "ls.json")]);
        update_store(&f.context, STORE, |s| {
            s["counter"] = json!(3);
            Ok(())
        })
        .await
        .unwrap();
        let store = initialize(&f.context).await.unwrap();
        assert_eq!(store["counter"], 3);
        assert_eq!(store[MARKER]["source"], Value::Null);

        let empty = tempfile::tempdir().unwrap();
        let g = fixture(empty.path(), &[(NATIVE_STORE, "missing.json")]);
        let store = initialize(&g.context).await.unwrap();
        assert!(store.get("counter").is_none());
        assert!(store.get(MARKER).is_some());
    }

    #[tokio::test]
    async fn malformed_source_is_not_imported_and_retried_later() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("native.json"), b"{broken").unwrap();
        let f = fixture(dir.path(), &[(NATIVE_STORE, "native.json")]);
        assert_eq!(initialize(&f.context).await.unwrap(), json!({}));
        assert!(!dir.path().join("data").exists());
        std::fs::write(dir.path().join("native.json"), br#"{"counter":2}"#).unwrap();
        assert_eq!(initialize(&f.context).await.unwrap()["counter"], 2);
    }
}
