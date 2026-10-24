//! Sessionless counter commands; persistence and transaction serialization belong to the host.
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde_json::{json, Value};
use std::{future::Future, pin::Pin};

pub type HostFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;
pub type StoreChange = Box<dyn FnOnce(&mut Value) -> Result<(), String> + Send + 'static>;

/// Both methods must target the same isolated namespace. `update_store` must
/// atomically read, apply, and persist across all contexts; errors must roll back.
/// Return `{ "ok": true, "store": <namespace object> }` on success. Keeping the
/// namespace inside this envelope permits arbitrary unknown keys, including `ok`.
pub trait Host: Sync {
    fn read_store(&self) -> HostFuture<'_>;
    fn update_store(&self, change: StoreChange) -> HostFuture<'_>;
}

pub const ICON: &[u8] = include_bytes!("../assets/command-icon.png");
pub const MANIFEST: &str = include_str!("../manifest.json");

pub fn commands() -> &'static [&'static str] {
    &["increment-counter", "decrement-counter", "reset-counter"]
}

fn checked_store(value: Value) -> Result<Value, String> {
    if value["ok"] == false {
        return Err(value["error"]
            .as_str()
            .or_else(|| value["message"].as_str())
            .unwrap_or("Host storage operation failed")
            .to_owned());
    }
    if value["ok"] != true || !value["store"].is_object() {
        return Err("Host must return ok:true with a Counter storage object".into());
    }
    Ok(value["store"].clone())
}

fn whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
        '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// JavaScript parseInt's default-radix numeric prefix and binary64 result.
fn parse_integer(text: &str) -> Result<f64, String> {
    let text = text.trim_start_matches(whitespace);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let hex = text.starts_with("0x") || text.starts_with("0X");
    let text = if hex { &text[2..] } else { text };
    let end = text
        .bytes()
        .take_while(|c| {
            if hex {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            }
        })
        .count();
    if end == 0 {
        return Err("Stored counter has no integer numeric prefix".into());
    }
    let magnitude = if hex {
        BigUint::parse_bytes(&text.as_bytes()[..end], 16)
            .and_then(|n| n.to_f64())
            .unwrap_or(f64::INFINITY)
    } else {
        text[..end].parse::<f64>().map_err(|e| e.to_string())?
    };
    let number = if negative { -magnitude } else { magnitude };
    if !number.is_finite() {
        return Err("Stored counter parses to a non-finite number".into());
    }
    Ok(number)
}

fn counter(store: &Value) -> Result<f64, String> {
    match store.get("counter") {
        None | Some(Value::Null) => Ok(0.0),
        Some(Value::String(text)) => parse_integer(text),
        Some(Value::Number(number)) => {
            let number = number.as_f64().ok_or("Counter is not a finite number")?;
            if !number.is_finite() {
                return Err("Counter is not a finite number".into());
            }
            // JS parseInt first applies Number.toString, including scientific notation.
            parse_integer(ryu_js::Buffer::new().format(number))
        }
        _ => Err("Stored counter must be a number, string, or null".into()),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|v| v != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Future host wiring may consume this metadata; this function sends no RPC.
pub fn command_metadata(value: f64) -> Result<Value, String> {
    if !value.is_finite() {
        return Err("Counter result is not finite".into());
    }
    Ok(json!({"subtitle": format!("Value: {}", ryu_js::Buffer::new().format(value))}))
}

pub async fn handle(
    host: &impl Host,
    command: &str,
    operation: &str,
    payload: &Value,
    _state: &mut Value,
) -> Result<Value, String> {
    if !commands().contains(&command) {
        return Err(format!("Unknown Counter command: {command}"));
    }
    if operation == "extension:dispose-session" {
        return Ok(json!({"ok":true,"mode":"unchanged"}));
    }
    if operation != "extension:run-command" {
        return Err(format!("Unsupported Counter operation: {operation}"));
    }
    let reset = command == "reset-counter";
    let no_update = !reset && truthy(&payload["launchContext"]["noUpdate"]);
    let store = if no_update {
        checked_store(host.read_store().await?)?
    } else {
        let delta = if command == "increment-counter" {
            1.0
        } else {
            -1.0
        };
        checked_store(
            host.update_store(Box::new(move |store| {
                if !store.is_object() {
                    return Err("Counter storage must be a JSON object".into());
                }
                let number = if reset { 0.0 } else { counter(store)? + delta };
                if !number.is_finite() {
                    return Err("Counter result is not finite".into());
                }
                store["counter"] = json!(number);
                Ok(())
            }))
            .await?,
        )?
    };
    // A transaction returns the committed snapshot. Do not parseInt it again:
    // e.g. incrementing 1e20 yields 1e20, whose JS string still has no exponent.
    let value = if no_update {
        counter(&store)?
    } else {
        store["counter"]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or("Host did not return the committed counter")?
    };
    let metadata = command_metadata(value)?;
    Ok(json!({"ok":true,"mode":"no-view","message":metadata["subtitle"],"value":value}))
}
