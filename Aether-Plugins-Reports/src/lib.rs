//! Structured report policy. Diagnostics never grant permission to repair media.
use media_extension_sdk::{ExtensionRequest, ExtensionResponse};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn manifest() -> Value {
    json!({"apiVersion":1,"id":"media-reports","name":"Signalements",
        "version":env!("CARGO_PKG_VERSION"),"category":"Applications",
        "description":"Signalements précis liés aux médias, pistes et instants de lecture, avec priorité et vérifications suggérées à l’administration. Suivi natif ; aucune réparation automatique.",
        "author":"Aether Team","license":"AGPL-3.0-only",
        "repository":"https://github.com/maelmoreau21/Aether-Plugins",
        "permissions":["reports"],"httpHosts":[],"targetAbi":"1.0",
        "minServerVersion":"0.1.0","banner":"assets/banner.svg"})
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Context {
    engine: Option<String>,
    error_code: Option<String>,
    audio_track_id: Option<String>,
    subtitle_track_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    category: String,
    #[serde(default)]
    position_seconds: Option<f64>,
    #[serde(default)]
    context: Context,
}

fn category_code(category: &str) -> Result<&'static str, &'static str> {
    match category {
        "video" => Ok("media.video"),
        "audio" => Ok("media.audio"),
        "subtitles" => Ok("media.subtitles"),
        "metadata" => Ok("media.metadata"),
        "missing" => Ok("media.missing"),
        "other" => Ok("media.other"),
        _ => Err("invalid_category"),
    }
}

pub fn handle_extension(request: ExtensionRequest) -> ExtensionResponse {
    let result = if request.api_version != 1 {
        json!({"error":"unsupported_api_version"})
    } else if !matches!(
        request.operation.as_str(),
        "reports.validate" | "reports.classify"
    ) {
        json!({"error":"unsupported_operation"})
    } else {
        match serde_json::from_value::<Input>(request.input) {
            Err(_) => json!({"valid":false,"error":"invalid_input"}),
            Ok(input) => match category_code(&input.category) {
                Err(error) => json!({"valid":false,"error":error}),
                // Core owns structural report validation; keep reports.validate as a minimal
                // compatibility response for older Cores.
                Ok(_) if request.operation == "reports.validate" => json!({"valid":true}),
                Ok(code) => {
                    let (priority, checks) = triage(&input);
                    json!({"category":input.category,"code":code,"priority":priority,
                        "checks":checks,"automationEligible":false})
                }
            },
        }
    };
    ExtensionResponse {
        api_version: 1,
        result,
    }
}

/// Administrator triage hint: it orders the queue and names what to verify first.
/// It never schedules work, and free text is never interpreted.
fn triage(input: &Input) -> (&'static str, Vec<&'static str>) {
    let failed = input.context.error_code.is_some();
    let located = input.position_seconds.is_some();
    let mut checks = Vec::new();
    let priority = match input.category.as_str() {
        "missing" => {
            checks.extend(["file_presence", "library_scan"]);
            "high"
        }
        "video" => {
            checks.push("video_decode");
            if input.context.engine.is_some() {
                checks.push("playback_engine");
            }
            if failed { "high" } else { "normal" }
        }
        "audio" => {
            checks.push(if input.context.audio_track_id.is_some() {
                "audio_track"
            } else {
                "audio_default_track"
            });
            if failed { "high" } else { "normal" }
        }
        "subtitles" => {
            checks.push(if input.context.subtitle_track_id.is_some() {
                "subtitle_track"
            } else {
                "subtitle_selection"
            });
            if located {
                checks.push("subtitle_timing");
            }
            "normal"
        }
        "metadata" => {
            checks.push("metadata_match");
            "low"
        }
        _ => "normal",
    };
    if located && matches!(input.category.as_str(), "video" | "audio" | "other") {
        checks.push("timestamp");
    }
    (priority, checks)
}

media_extension_sdk::extension!(handle_extension);

#[cfg(test)]
mod tests {
    use super::*;
    fn run(operation: &str, input: Value) -> Value {
        handle_extension(ExtensionRequest {
            api_version: 1,
            operation: operation.into(),
            input,
        })
        .result
    }
    fn report() -> Value {
        json!({"category":"audio","description":"Audio interrupted","workId":"work-1",
            "assetId":"asset-2","positionSeconds":12.5,"endSeconds":13.0,
            "context":{"engine":"native","audioTrackId":"track-3","errorCode":"decode_error"}})
    }
    #[test]
    fn precise_report_is_valid_and_classification_never_authorizes_repairs() {
        assert_eq!(run("reports.validate", report()), json!({"valid":true}));
        assert_eq!(
            run("reports.classify", report()),
            json!({"category":"audio","code":"media.audio","priority":"high",
                "checks":["audio_track","timestamp"],"automationEligible":false})
        );
    }
    #[test]
    fn triage_orders_the_queue_without_reading_free_text() {
        let missing = json!({"category":"missing","description":"urgent!!! high","workId":"w"});
        let result = run("reports.classify", missing);
        assert_eq!(result["priority"], "high");
        assert_eq!(result["checks"], json!(["file_presence", "library_scan"]));
        let metadata = json!({"category":"metadata","description":"URGENT","workId":"w"});
        assert_eq!(run("reports.classify", metadata)["priority"], "low");
        let subtitles = json!({"category":"subtitles","description":"Late","workId":"w",
            "assetId":"a","positionSeconds":3.0,"context":{"subtitleTrackId":"s"}});
        let result = run("reports.classify", subtitles);
        assert_eq!(result["priority"], "normal");
        assert_eq!(
            result["checks"],
            json!(["subtitle_track", "subtitle_timing"])
        );
        let video = json!({"category":"video","description":"Frozen","trackId":"t",
            "context":{"engine":"native"}});
        let result = run("reports.classify", video);
        assert_eq!(result["priority"], "normal");
        assert_eq!(result["checks"], json!(["video_decode", "playback_engine"]));
        assert_eq!(result["automationEligible"], false);
    }
    #[test]
    fn accepts_core_valid_context_and_rejects_unstructured_context() {
        let mut input = report();
        input["context"]["engine"] = json!("native player");
        assert_eq!(run("reports.classify", input)["priority"], "high");

        let mut input = report();
        input["context"]["logs"] = json!("private path and token");
        assert_eq!(run("reports.classify", input)["error"], "invalid_input");
    }
    #[test]
    fn rejects_invalid_payload_types_and_unknown_category() {
        let mut input = report();
        input["positionSeconds"] = json!("12");
        assert_eq!(run("reports.classify", input)["valid"], false);
        let mut input = report();
        input["category"] = json!("repair");
        assert_eq!(run("reports.classify", input)["error"], "invalid_category");
    }
}
