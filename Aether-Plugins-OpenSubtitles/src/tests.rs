//! Offline contracts. Fixtures under `tests/corpus` have the shape of real REST answers; no test
//! contacts OpenSubtitles.
use super::*;

const HASH_AND_DUPLICATES: &str = include_str!("../tests/corpus/search_hash_and_duplicates.json");
const LANGUAGE_VARIANTS: &str = include_str!("../tests/corpus/search_language_variants.json");

fn input(value: Value) -> Input {
    serde_json::from_value(value).unwrap()
}

fn query_of(value: Value) -> (String, &'static str) {
    search_query(&input(value)).unwrap()
}

fn error_of(value: Value) -> &'static str {
    search_query(&input(value)).unwrap_err()
}

fn wanted<'a>(language: &'a str, forced: Option<bool>, sdh: Option<bool>) -> Wanted<'a> {
    Wanted {
        language,
        forced,
        sdh,
    }
}

fn ids(result: &Value) -> Vec<&str> {
    result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect()
}

#[test]
fn identity_query_does_not_guess_title_or_hash() {
    let (query, basis) = query_of(
        json!({"title":"Wrong title","year":2020,"mediaType":"movie","language":"fr","externalIds":{"imdb":"tt0133093"},"forced":true,"sdh":false}),
    );
    assert_eq!(basis, "identity");
    assert_eq!(
        query,
        "foreign_parts_only=only&hearing_impaired=exclude&imdb_id=133093&languages=fr&type=movie"
    );
    assert!(!query.contains("moviehash"));
}

#[test]
fn fallback_query_and_invalid_inputs() {
    let (query, basis) =
        query_of(json!({"title":"A Film","year":1999,"mediaType":"movie","language":"en"}));
    assert_eq!(basis, "title");
    assert!(query.contains("query=a+film"));
    assert!(query.contains("year=1999"));
    for value in [
        json!({"mediaType":"movie","language":"../en"}),
        json!({"title":"x","mediaType":"series","language":"fr"}),
        json!({"title":"x","mediaType":"movie","language":"fr","page":0}),
        json!({"title":"x","mediaType":"movie","language":"fr","page":101}),
    ] {
        assert!(search_query(&input(value)).is_err());
    }
}

#[test]
fn movie_hash_is_the_provider_hash_never_aether_fingerprint() {
    let base =
        |hash: &str| json!({"title":"A Film","mediaType":"movie","language":"fr","movieHash":hash});
    // Valid: 16 hex digits, any case, surrounding blanks ignored.
    let (query, basis) = query_of(base(" 8E245D9679D31E12 "));
    assert!(query.contains("moviehash=8e245d9679d31e12"));
    // A hash designates the file: no title and no weaker criterion is added.
    assert!(!query.contains("query="));
    assert_eq!(basis, "identity");
    // Aether's BLAKE3 sample fingerprint and malformed values are refused, never forwarded.
    for bad in [
        "b3s1:1073741824:0123456789abcdef0123456789abcdef",
        "b3f1:4096:0123456789abcdef0123456789abcdef",
        "8e245d9679d31e1",
        "8e245d9679d31e123",
        "8e245d9679d31e1g",
        "0000000000000000",
        "../../etc/passwd",
    ] {
        assert_eq!(error_of(base(bad)), "invalid_movie_hash", "{bad}");
    }
    // A blank hash is absent, not an error: the title search still works.
    let (query, basis) = query_of(base(""));
    assert_eq!(basis, "title");
    assert!(!query.contains("moviehash"));
}

#[test]
fn hash_can_be_combined_with_identifiers_and_required() {
    let (query, _) = query_of(
        json!({"mediaType":"movie","language":"fr","movieHash":"8e245d9679d31e12","externalIds":{"imdb":"tt0133093"},"requireHash":true}),
    );
    assert!(query.contains("moviehash=8e245d9679d31e12"));
    assert!(query.contains("moviehash_match=only"));
    assert!(query.contains("imdb_id=133093"));
    // Requiring a hash without having one must not fall back to a looser search.
    assert_eq!(
        error_of(json!({"title":"x","mediaType":"movie","language":"fr","requireHash":true})),
        "movie_hash_required"
    );
}

#[test]
fn blank_identifiers_never_shadow_usable_ones() {
    let (query, basis) = query_of(
        json!({"mediaType":"movie","language":"fr","externalIds":{"imdb":"  ","tmdb":"603"}}),
    );
    assert!(query.contains("tmdb_id=603") && !query.contains("imdb_id"));
    assert_eq!(basis, "identity");
    // Present but malformed is an error, not a quiet downgrade to the title.
    assert_eq!(
        error_of(
            json!({"title":"x","mediaType":"movie","language":"fr","externalIds":{"imdb":"tt12ab"}})
        ),
        "invalid_external_id"
    );
    assert_eq!(
        error_of(
            json!({"title":"x","mediaType":"movie","language":"fr","externalIds":{"tmdb":"0"}})
        ),
        "invalid_external_id"
    );
}

#[test]
fn episodes_use_their_own_ids_then_series_ids_with_numbers() {
    // Episode-level identifier: used as is, numbers kept as extra context.
    let (query, basis) = query_of(
        json!({"mediaType":"episode","language":"fr","externalIds":{"imdb":"tt0959621"},"seasonNumber":1,"episodeNumber":2}),
    );
    assert!(query.contains("imdb_id=959621"));
    assert!(query.contains("season_number=1") && query.contains("episode_number=2"));
    assert_eq!(basis, "identity");
    // Series identifiers need both numbers.
    let (query, basis) = query_of(
        json!({"mediaType":"episode","language":"fr","parentExternalIds":{"imdb":"tt0903747"},"seasonNumber":0,"episodeNumber":3}),
    );
    assert!(query.contains("parent_imdb_id=903747"));
    assert!(query.contains("season_number=0") && query.contains("episode_number=3"));
    assert_eq!(basis, "identity");
    assert_eq!(
        error_of(
            json!({"mediaType":"episode","language":"fr","parentExternalIds":{"tmdb":"1396"},"seasonNumber":1})
        ),
        "season_episode_required"
    );
    // Series title: the work's year is the series', so it is not sent; numbers are mandatory.
    let (query, basis) = query_of(
        json!({"title":"Breaking Bad","year":2008,"mediaType":"episode","language":"fr","seasonNumber":2,"episodeNumber":5}),
    );
    assert_eq!(basis, "title");
    assert!(!query.contains("year="));
    assert_eq!(
        error_of(
            json!({"title":"Breaking Bad","mediaType":"episode","language":"fr","episodeNumber":5})
        ),
        "season_episode_required"
    );
    assert_eq!(
        error_of(
            json!({"title":"x","mediaType":"episode","language":"fr","seasonNumber":10000,"episodeNumber":1})
        ),
        "invalid_episode_number"
    );
    // A movie never carries numbers it was handed by mistake.
    let (query, _) = query_of(
        json!({"title":"x","mediaType":"movie","language":"fr","seasonNumber":1,"episodeNumber":1}),
    );
    assert!(!query.contains("season_number") && !query.contains("episode_number"));
}

#[test]
fn languages_are_normalised_and_regional_variants_kept() {
    for (given, sent) in [
        ("fr", "fr"),
        ("pt-BR", "pt-br"),
        ("PT_br", "pt-br"),
        (" zh-CN ", "zh-cn"),
        ("zh-Hant", "zh-hant"),
        ("es-419", "es-419"),
        ("fil", "fil"),
    ] {
        let (query, _) = query_of(json!({"title":"x","mediaType":"movie","language":given}));
        assert!(
            query.contains(&format!("languages={sent}&"))
                || query.contains(&format!("languages={sent}")),
            "{given}: {query}"
        );
        assert_eq!(normalize_language(given).as_deref(), Some(sent));
    }
    for bad in [
        "", "f", "french", "fr-", "fr-b", "pt-br-x", "fr,en", "fr/../", "1a", "fr-ça",
    ] {
        assert_eq!(normalize_language(bad), None, "{bad}");
    }
}

#[test]
fn search_hash_matches_first_dedup_and_fallback_labels() {
    let value: Value = serde_json::from_str(HASH_AND_DUPLICATES).unwrap();
    let result = search_result(value, "identity", &wanted("fr", None, None)).unwrap();
    // 103 is English (asked: French), 106 has no language, the last entry has no valid id, the
    // repeated 101 is dropped, and the file matching the caller's hash comes first.
    assert_eq!(ids(&result), ["102", "101", "104", "105"]);
    assert_eq!(result["items"][0]["hashMatch"], true);
    assert_eq!(result["items"][1]["hashMatch"], false);
    // A release made of control characters falls back to the file name, never an empty label.
    assert_eq!(result["items"][3]["label"], "fallback.name.srt");
    // The page count stays within what the Core accepts.
    assert_eq!(result["totalPages"], 100);
}

#[test]
fn regional_variants_are_respected_and_numeric_regions_stay_valid_for_the_core() {
    let load = || serde_json::from_str::<Value>(LANGUAGE_VARIANTS).unwrap();
    let all = |l: &str| search_result(load(), "identity", &wanted(l, None, None)).unwrap();
    assert_eq!(ids(&all("pt-br")), ["201", "203", "204", "208"]);
    assert_eq!(ids(&all("pt-pt")), ["202"]);
    // The generic code accepts every variant, the generic file included.
    assert_eq!(ids(&all("pt")), ["201", "202", "203", "204", "205", "208"]);
    // `es-419` would make the Core refuse the whole page: only the primary language is exposed.
    let spanish = all("es");
    assert_eq!(ids(&spanish), ["206"]);
    assert_eq!(spanish["items"][0]["language"], "es");
    assert_eq!(all("zh-cn")["items"][0]["language"], "zh-cn");
    // The provider's underscore spelling is normalised.
    assert_eq!(all("pt-br")["items"][3]["language"], "pt-br");
    assert_eq!(all("pt-br")["items"][3]["machineTranslated"], true);
}

#[test]
fn forced_and_sdh_are_enforced_even_if_the_provider_ignores_them() {
    let load = || serde_json::from_str::<Value>(LANGUAGE_VARIANTS).unwrap();
    let run = |forced, sdh| {
        let result = search_result(load(), "identity", &wanted("pt-br", forced, sdh)).unwrap();
        ids(&result)
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    // No preference: everything, flags reported.
    assert_eq!(run(None, None), ["201", "203", "204", "208"]);
    // Complete: neither forced nor SDH.
    assert_eq!(run(Some(false), Some(false)), ["201", "208"]);
    assert_eq!(run(Some(true), None), ["204"]);
    assert_eq!(run(None, Some(true)), ["203"]);
    assert!(run(Some(true), Some(true)).is_empty());
    let all = search_result(load(), "identity", &wanted("pt-br", None, None)).unwrap();
    assert_eq!(all["items"][1]["sdh"], true);
    assert_eq!(all["items"][2]["forced"], true);
}

#[test]
fn search_flattens_files_and_never_returns_provider_private_data() {
    let value = json!({"total_pages":3,"data":[{"attributes":{"language":"fr","foreign_parts_only":true,"hearing_impaired":true,"release":"Release","uploader":{"name":"private"},"url":"https://private.example/x","files":[{"file_id":42},{"file_id":43}]}}]});
    let result = search_result(value, "identity", &wanted("fr", None, None)).unwrap();
    assert_eq!(ids(&result), ["42", "43"]);
    assert_eq!(result["items"][0]["forced"], true);
    assert_eq!(result["items"][0]["sdh"], true);
    assert!(!result.to_string().contains("private"));
    assert!(
        search_result(
            json!({"secret":"never returned"}),
            "title",
            &wanted("fr", None, None)
        )
        .is_err()
    );
    // More than a page of candidates is cut at what the Core accepts.
    let many: Vec<Value> = (1..=80)
        .map(|n| json!({"attributes":{"language":"fr","release":"r","files":[{"file_id":n}]}}))
        .collect();
    let result = search_result(json!({"data":many}), "title", &wanted("fr", None, None)).unwrap();
    assert!(result["items"].as_array().unwrap().len() <= MAX_ITEMS);
}

#[test]
fn download_hosts_and_quotas_are_validated() {
    let output=download_result(json!({"link":"https://dl.opensubtitles.com/download/opaque","remaining":0,"reset_time_utc":"2026-11-04T00:00:00Z","token":"secret"})).unwrap();
    assert_eq!(output["remaining"], 0);
    assert_eq!(output["resetTimeUtc"], "2026-11-04T00:00:00Z");
    assert!(output.get("token").is_none());
    // A link without quota information is still usable; nothing is invented.
    let output = download_result(json!({"link":"https://www.opensubtitles.com/x"})).unwrap();
    assert!(output.get("remaining").is_none() && output.get("resetTimeUtc").is_none());
    for link in [
        "http://dl.opensubtitles.com/x",
        "https://dl.opensubtitles.com.evil/x",
        "https://127.0.0.1/x",
        "https://dl.opensubtitles.com@evil/x",
        "https://dl.opensubtitles.com:8443/x",
        "https://dl.opensubtitles.com/a b",
        "https://dl.opensubtitles.com/a\\..\\b",
        "https://dl.opensubtitles.com/a\r\nHost: evil",
        "",
    ] {
        assert!(download_result(json!({"link":link})).is_err(), "{link:?}");
    }
    assert!(download_result(json!({"message":"no link"})).is_err());
}

#[test]
fn documented_statuses_map_to_stable_codes() {
    use Call::*;
    for (status, call, code) in [
        (200, Search, None),
        (401, Login, Some("invalid_credentials")),
        (401, Search, Some("unauthorized")),
        (403, Download, Some("unauthorized")),
        (404, Download, Some("not_found")),
        (410, Download, Some("not_found")),
        (400, Search, Some("invalid_request")),
        (422, Search, Some("invalid_request")),
        // 406 on a download is the documented daily allowance; elsewhere it is not.
        (406, Download, Some("download_quota_exceeded")),
        (406, Search, Some("provider_error")),
        (408, Search, Some("timeout")),
        (429, Search, Some("rate_limited")),
        (503, Search, Some("rate_limited")),
        (502, Search, Some("provider_unavailable")),
        (504, Download, Some("provider_unavailable")),
        (302, Search, Some("provider_redirect_refused")),
        (500, Search, Some("provider_error")),
    ] {
        assert_eq!(status_code(status, call), code, "{status}");
    }
}

#[tokio::test]
async fn invalid_calls_never_need_network_or_reveal_secrets() {
    let req = |op: &str, input| ExtensionRequest {
        api_version: 1,
        operation: op.into(),
        input,
    };
    assert_eq!(
        run(req("subtitles.search", json!({}))).await.result,
        json!({"error":"missing_api_key"})
    );
    let result = run(req(
        "subtitles.download",
        json!({"fileId":"../secret","config":{"api_key":"fake-secret"}}),
    ))
    .await
    .result;
    assert_eq!(result, json!({"error":"invalid_file_id"}));
    assert!(!result.to_string().contains("fake-secret"));
    // Invalid search criteria are caught before any request (no server is reachable here).
    for (input, code) in [
        (
            json!({"mediaType":"movie","language":"fr","title":"x","movieHash":"b3s1:9:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","config":{"api_key":"k"}}),
            "invalid_movie_hash",
        ),
        (
            json!({"mediaType":"episode","language":"fr","title":"x","config":{"api_key":"k"}}),
            "season_episode_required",
        ),
        (
            json!({"mediaType":"movie","language":"french","title":"x","config":{"api_key":"k"}}),
            "invalid_language",
        ),
    ] {
        assert_eq!(
            run(req("subtitles.search", input)).await.result,
            json!({"error":code})
        );
    }
}
