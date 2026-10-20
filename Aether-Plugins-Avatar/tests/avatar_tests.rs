use aether_plugin_api::{
    API_VERSION, MetadataProviderPlugin, PluginRequest, PluginResponse, ProviderConfig,
    StructuredMetadata, handle_request,
};
use aether_plugin_avatar::AvatarPlugin;

#[test]
fn test_avatar_manifest() {
    let plugin = AvatarPlugin::new();
    let manifest = plugin.manifest();
    assert_eq!(manifest.id, "avatars");
    assert_eq!(manifest.category, "Avatars");
    assert_eq!(manifest.target_abi, "1.0");
    assert!(manifest.banner.is_some());
    assert!(
        manifest
            .config_schema
            .iter()
            .any(|f| f.key == "disallow_custom_avatars")
    );
    assert!(manifest.config_schema.iter().any(|f| f.key == "pack_name"));
    assert!(
        manifest
            .config_schema
            .iter()
            .any(|f| f.key == "force_random_on_creation")
    );
}

#[test]
fn test_avatar_packs_and_items() {
    let plugin = AvatarPlugin::new();
    let packs = plugin.packs();
    assert_eq!(packs.len(), 4);
    assert!(packs.iter().any(|p| p.id == "personas"));
    assert!(packs.iter().any(|p| p.id == "geometric"));
    assert!(packs.iter().any(|p| p.id == "retro"));
    assert!(packs.iter().any(|p| p.id == "botts"));

    let avatars = plugin.get_pack_avatars("personas");
    assert_eq!(avatars.len(), 10);
    assert!(avatars[0].svg.starts_with("<svg"));
    assert!(avatars[0].svg.ends_with("</svg>"));
}

#[test]
fn test_random_avatar_deterministic_seed() {
    let plugin = AvatarPlugin::new();
    let av1 = plugin.random_avatar(Some("geometric"), Some("user_alice"));
    let av2 = plugin.random_avatar(Some("geometric"), Some("user_alice"));
    assert_eq!(av1.id, av2.id);

    let av3 = plugin.random_avatar(Some("geometric"), Some("user_bob"));
    // Different seed should generally yield different avatar index
    assert!(!av3.svg.is_empty());
}

#[tokio::test]
async fn test_avatar_ipc_dispatch() {
    let plugin = AvatarPlugin::new();
    let req = PluginRequest {
        api_version: API_VERSION,
        operation: "details".into(),
        input: serde_json::json!({
            "id": "personas",
            "mediaType": "movie"
        }),
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<StructuredMetadata> = serde_json::from_str(&res_str).unwrap();

    assert!(res.success);
    let details = res.data.unwrap();
    assert_eq!(details.provider, "aether-avatars");
    assert_eq!(details.images.len(), 10);
    assert!(
        details.images[0]
            .url
            .starts_with("data:image/svg+xml;utf8,")
    );
}
