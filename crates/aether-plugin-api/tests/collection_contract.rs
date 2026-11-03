use aether_plugin_api::{MetadataCollection, StructuredMetadata};
use serde_json::json;

#[test]
fn collection_contract_is_reexported_and_survives_serialization() {
    let metadata = StructuredMetadata {
        provider: "tmdb".into(),
        provider_id: "42".into(),
        collection: Some(MetadataCollection {
            id: "tmdb:7".into(),
            name: "Fixture collection".into(),
        }),
        ..Default::default()
    };
    let wire = serde_json::to_value(&metadata).unwrap();
    assert_eq!(
        wire["collection"],
        json!({"id": "tmdb:7", "name": "Fixture collection"})
    );
    assert_eq!(
        serde_json::from_value::<StructuredMetadata>(wire).unwrap(),
        metadata
    );
}

#[test]
fn metadata_without_collection_remains_compatible() {
    let legacy = json!({"provider": "fixture", "providerId": "42"});
    let metadata: StructuredMetadata = serde_json::from_value(legacy).unwrap();
    assert!(metadata.collection.is_none());
    assert!(
        serde_json::to_value(metadata)
            .unwrap()
            .get("collection")
            .is_none()
    );
}
