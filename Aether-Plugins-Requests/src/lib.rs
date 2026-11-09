//! Request policy runs in Wasm; the Core owns connections, credentials and durable effects.
use media_extension_sdk::{ExtensionRequest, ExtensionResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn manifest() -> Value {
    json!({"apiVersion":1,"id":"media-requests","name":"Demandes",
        "version":env!("CARGO_PKG_VERSION"),"category":"Applications",
        "description":"Demandes de films, séries et musique avec validation et suivi natifs. Nécessite le contrat hôte requests ; connexions gérées par le Core.",
        "author":"Aether Team","license":"AGPL-3.0-only",
        "repository":"https://github.com/maelmoreau21/Aether-Plugins",
        "permissions":["requests"],"httpHosts":[],"targetAbi":"1.0",
        "minServerVersion":"0.1.0","banner":"assets/banner.svg"})
}

#[derive(Clone, Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Episode {
    season: u32,
    episode: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    kind: String,
    title: String,
    external_id: String,
    external_provider: String,
    #[serde(default)]
    seasons: Vec<u32>,
    #[serde(default)]
    episodes: Vec<Episode>,
    #[serde(default)]
    album_ids: Vec<String>,
    #[serde(default)]
    monitor_future: bool,
    /// Host-only evidence: Core never accepts these fields from request creation.
    #[serde(default)]
    resolved_single_id: Option<String>,
    #[serde(default)]
    verified_recording_id: Option<String>,
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn numeric(value: &str) -> bool {
    value.len() <= 16
        && !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte != b'0')
}

fn validate(input: &Input) -> Result<&'static str, &'static str> {
    if input.title.trim().is_empty()
        || input.title.len() > 512
        || input.title.chars().any(char::is_control)
    {
        return Err("invalid_title");
    }
    let service = match input.kind.as_str() {
        "film" if input.external_provider == "tmdb" && numeric(&input.external_id) => "radarr",
        "series"
            if matches!(input.external_provider.as_str(), "tmdb" | "tvdb")
                && numeric(&input.external_id) =>
        {
            "sonarr"
        }
        "artist" | "album" | "track"
            if input.external_provider == "musicbrainz" && uuid(&input.external_id) =>
        {
            "lidarr"
        }
        "film" | "series" | "artist" | "album" | "track" => {
            return Err("invalid_external_identity");
        }
        _ => return Err("invalid_kind"),
    };
    if input.seasons.len() > 1000
        || input.seasons.iter().any(|number| *number > 10000)
        || input.seasons.iter().collect::<BTreeSet<_>>().len() != input.seasons.len()
        || input.episodes.len() > 1000
        || input
            .episodes
            .iter()
            .any(|item| item.season > 10000 || item.episode == 0 || item.episode > 100000)
        || input.episodes.iter().collect::<BTreeSet<_>>().len() != input.episodes.len()
        || input.album_ids.len() > 1000
        || input.album_ids.iter().any(|id| !uuid(id))
        || input.album_ids.iter().collect::<BTreeSet<_>>().len() != input.album_ids.len()
    {
        return Err("invalid_selection");
    }
    if input.kind != "series" && (!input.seasons.is_empty() || !input.episodes.is_empty()) {
        return Err("invalid_selection");
    }
    if input.kind != "artist" && !input.album_ids.is_empty() {
        return Err("invalid_selection");
    }
    if input.monitor_future && !matches!(input.kind.as_str(), "series" | "artist") {
        return Err("invalid_monitoring");
    }
    // An artist request without albums or future releases would add nothing.
    if input.kind == "artist" && input.album_ids.is_empty() && !input.monitor_future {
        return Err("empty_selection");
    }
    Ok(service)
}

/// Canonical scope: sorted, and episodes already covered by a whole season dropped.
/// The acquisition effect is unchanged; the Core and the interface show one form.
fn scope(input: &Input) -> (Vec<u32>, Vec<Episode>, &'static str) {
    let seasons: BTreeSet<u32> = input.seasons.iter().copied().collect();
    let episodes: Vec<Episode> = input
        .episodes
        .iter()
        .filter(|item| !seasons.contains(&item.season))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let label = match input.kind.as_str() {
        "series" => match (seasons.is_empty(), episodes.is_empty()) {
            (true, true) => "all",
            (false, true) => "seasons",
            (true, false) => "episodes",
            (false, false) => "seasons_and_episodes",
        },
        "artist" => match (input.album_ids.is_empty(), input.monitor_future) {
            (false, true) => "albums_and_future",
            (false, false) => "albums",
            _ => "future",
        },
        _ => "all",
    };
    (seasons.into_iter().collect(), episodes, label)
}

pub fn handle_extension(request: ExtensionRequest) -> ExtensionResponse {
    let result = if request.api_version != 1 {
        json!({"error":"unsupported_api_version"})
    } else if !matches!(
        request.operation.as_str(),
        "requests.validate" | "requests.plan"
    ) {
        json!({"error":"unsupported_operation"})
    } else {
        match serde_json::from_value::<Input>(request.input) {
            Err(_) => json!({"valid":false,"error":"invalid_input"}),
            Ok(input) => match validate(&input) {
                Err(error) => json!({"valid":false,"error":error}),
                Ok(_) if request.operation == "requests.validate" => json!({"valid":true}),
                // Only the host may prove a one-track Single matching this recording.
                Ok(_) if input.kind == "track" => {
                    match (&input.resolved_single_id, &input.verified_recording_id) {
                        (Some(single), Some(recording))
                            if uuid(single) && recording == &input.external_id =>
                        {
                            json!({"service":"lidarr","kind":"track","externalId":input.external_id,"externalProvider":"musicbrainz","acquisitionKind":"album","acquisitionExternalId":single})
                        }
                        _ => json!({"error":"track_not_supported"}),
                    }
                }
                Ok(service) => {
                    let (seasons, episodes, label) = scope(&input);
                    json!({"service":service,"kind":input.kind,
                    "title":input.title.trim(),"externalId":input.external_id,
                    "externalProvider":input.external_provider,"seasons":seasons,
                    "episodes":episodes,"albumIds":input.album_ids,
                    "monitorFuture":input.monitor_future,"scope":label})
                }
            },
        }
    };
    ExtensionResponse {
        api_version: 1,
        result,
    }
}

media_extension_sdk::extension!(handle_extension);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_recording_plan_requires_host_proof_for_the_exact_recording() {
        let recording = "22345678-1234-1234-1234-123456789abc";
        let single = "12345678-1234-1234-1234-123456789abc";
        let mut input = json!({"kind":"track","title":"Synthetic single","externalId":recording,"externalProvider":"musicbrainz","resolvedSingleId":single,"verifiedRecordingId":recording});
        let plan = run("requests.plan", input.clone());
        assert_eq!(plan["kind"], "track");
        assert_eq!(plan["externalId"], recording);
        assert_eq!(plan["acquisitionKind"], "album");
        assert_eq!(plan["acquisitionExternalId"], single);
        input["verifiedRecordingId"] = json!(single);
        assert_eq!(run("requests.plan", input)["error"], "track_not_supported");
    }
    fn run(operation: &str, input: Value) -> Value {
        handle_extension(ExtensionRequest {
            api_version: 1,
            operation: operation.into(),
            input,
        })
        .result
    }
    fn request(kind: &str) -> Value {
        json!({"kind":kind,"title":"Example","externalId":"10","externalProvider":"tmdb"})
    }
    #[test]
    fn plan_selects_service_and_preserves_explicit_episode_scope() {
        assert_eq!(run("requests.plan", request("film"))["service"], "radarr");
        let mut series = request("series");
        series["seasons"] = json!([0, 2]);
        series["episodes"] = json!([{"season":1,"episode":3}]);
        let result = run("requests.plan", series);
        assert_eq!(result["service"], "sonarr");
        assert_eq!(result["seasons"], json!([0, 2]));
        assert_eq!(result["episodes"], json!([{"season":1,"episode":3}]));
        assert_eq!(result["monitorFuture"], false);
        assert_eq!(result["scope"], "seasons_and_episodes");
        assert_eq!(run("requests.plan", request("film"))["scope"], "all");
    }
    #[test]
    fn plan_scope_is_canonical_and_drops_episodes_covered_by_a_season() {
        let mut series = request("series");
        series["seasons"] = json!([3, 1]);
        series["episodes"] =
            json!([{"season":2,"episode":5},{"season":1,"episode":2},{"season":2,"episode":1}]);
        let result = run("requests.plan", series);
        assert_eq!(result["seasons"], json!([1, 3]));
        assert_eq!(
            result["episodes"],
            json!([{"season":2,"episode":1},{"season":2,"episode":5}])
        );
        let mut whole = request("series");
        whole["monitorFuture"] = json!(true);
        assert_eq!(run("requests.plan", whole)["scope"], "all");
    }
    #[test]
    fn artist_request_must_select_albums_or_future_releases() {
        let mbid = "01234567-89ab-cdef-0123-456789abcdef";
        let empty = json!({"kind":"artist","title":"Artist","externalProvider":"musicbrainz","externalId":mbid});
        assert_eq!(run("requests.validate", empty)["error"], "empty_selection");
        let future = json!({"kind":"artist","title":"Artist","externalProvider":"musicbrainz",
            "externalId":mbid,"monitorFuture":true});
        assert_eq!(run("requests.plan", future)["scope"], "future");
    }
    #[test]
    fn individual_track_is_valid_intent_but_never_plans_an_album() {
        let track = json!({"kind":"track","title":"A track","externalProvider":"musicbrainz",
            "externalId":"01234567-89ab-cdef-0123-456789abcdef"});
        assert_eq!(
            run("requests.validate", track.clone()),
            json!({"valid":true})
        );
        assert_eq!(
            run("requests.plan", track),
            json!({"error":"track_not_supported"})
        );
    }
    #[test]
    fn rejects_ambiguous_identity_duplicate_selection_and_unrequested_monitoring() {
        let mut invalid = request("film");
        invalid["externalId"] = json!("0");
        assert_eq!(run("requests.validate", invalid)["valid"], false);
        let mut invalid = request("series");
        invalid["seasons"] = json!([1, 1]);
        assert_eq!(
            run("requests.validate", invalid)["error"],
            "invalid_selection"
        );
        let mut invalid = request("film");
        invalid["monitorFuture"] = json!(true);
        assert_eq!(run("requests.plan", invalid)["error"], "invalid_monitoring");
    }
    #[test]
    fn artist_plan_only_forwards_selected_albums_and_never_credentials() {
        let mbid = "01234567-89ab-cdef-0123-456789abcdef";
        let input = json!({"kind":"artist","title":"Artist","externalProvider":"musicbrainz",
            "externalId":mbid,"albumIds":[mbid],"apiKey":"never-forward","serverId":"local"});
        let result = run("requests.plan", input);
        assert_eq!(result["service"], "lidarr");
        assert_eq!(result["albumIds"], json!([mbid]));
        assert_eq!(result["monitorFuture"], false);
        assert_eq!(result["scope"], "albums");
        assert!(result.get("apiKey").is_none());
        assert!(result.get("serverId").is_none());
    }
    #[test]
    fn rejects_unknown_protocol_operations() {
        assert_eq!(
            run("requests.delete", request("film"))["error"],
            "unsupported_operation"
        );
        let result = handle_extension(ExtensionRequest {
            api_version: 99,
            operation: "requests.validate".into(),
            input: request("film"),
        });
        assert_eq!(result.result["error"], "unsupported_api_version");
    }
}
