//! Legal synthetic guest: an invocation counter persists through worker/Core restarts.
use media_extension_sdk::{ExtensionRequest, ExtensionResponse, serde_json::json, storage::*};

fn counter() -> Result<String, StorageError> {
    let scope = StorageScope {
        kind: "global".into(),
        id: String::new(),
    };
    let current = host_storage(&StorageRequest {
        api_version: STORAGE_API_VERSION,
        scope: scope.clone(),
        operation: StorageOperation::Get {
            key: "invocations".into(),
        },
    })?;
    let entry = current.entries.first();
    let value = entry
        .and_then(|v| v.value.parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_add(1)
        .to_string();
    host_storage(&StorageRequest {
        api_version: STORAGE_API_VERSION,
        scope,
        operation: StorageOperation::Put {
            key: "invocations".into(),
            value: value.clone(),
            expected_version: entry.map(|v| v.version.clone()),
            expires_at: None,
            durability: "durable".into(),
        },
    })?;
    Ok(value)
}

pub fn invoke(_: ExtensionRequest) -> ExtensionResponse {
    let result = match counter() {
        Ok(count) => {
            json!({"title":"Synthetic persistent extension","summary":format!("Successful durable invocation {count}")})
        }
        Err(error) => json!({"title":"Synthetic storage error","summary":error.code}),
    };
    ExtensionResponse {
        api_version: 1,
        result,
    }
}
media_extension_sdk::extension!(invoke);
