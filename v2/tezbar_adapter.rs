//! Compile as native_core/extensions/v2_counter.rs; see README integration.
use super::v2_sdk;
use crate::native_core::common::{CoreContext, CoreResult};
use serde_json::{json, Value};
use tezbar_counter_v2::{Host, HostFuture, StoreChange};

const SLUG: &str = "counter";
struct TezbarHost<'a>(&'a CoreContext);
impl Host for TezbarHost<'_> {
    fn read_store(&self) -> HostFuture<'_> {
        Box::pin(async move {
            let store = v2_sdk::read_store(self.0, SLUG).await?;
            Ok(json!({"ok":true,"store":store}))
        })
    }
    fn update_store(&self, change: StoreChange) -> HostFuture<'_> {
        Box::pin(async move {
            let store = v2_sdk::update_store(self.0, SLUG, change).await?;
            Ok(json!({"ok":true,"store":store}))
        })
    }
}
pub fn commands() -> &'static [&'static str] {
    tezbar_counter_v2::commands()
}
pub const ICON: &[u8] = tezbar_counter_v2::ICON;
pub const MANIFEST: &str = tezbar_counter_v2::MANIFEST;
pub async fn handle(
    ctx: &CoreContext,
    command: &str,
    operation: &str,
    payload: &Value,
    state: &mut Value,
) -> CoreResult {
    tezbar_counter_v2::handle(&TezbarHost(ctx), command, operation, payload, state).await
}
