//! Poster rotation settings, inspired by the original Jellyfin Poster Rotator.
use media_extension_sdk::{
    ConfigField, ConfigOption, ExtensionRequest, ExtensionResponse, Manifest,
};
use serde_json::{Value, json};

fn field(key: &str, label: &str, description: &str, kind: &str, default: &str) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        field_type: kind.into(),
        default_value: Some(default.into()),
        required: false,
        secret: false,
        options: Vec::new(),
    }
}

pub fn manifest() -> Value {
    serde_json::to_value(Manifest {
        api_version: 1,
        id: "poster-rotator".into(),
        name: "Poster Rotator".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Rotation configurable des affiches des films et séries, par bibliothèque."
            .into(),
        author: "Aether Team".into(),
        license: "MIT".into(),
        repository: "https://github.com/maelmoreau21/Aether-Plugins".into(),
        permissions: vec!["images".into()],
        http_hosts: Vec::new(),
        category: Some("Artwork".into()),
        target_abi: Some("1.0".into()),
        min_server_version: Some("0.1.0".into()),
        max_server_version: Some("1.0.0".into()),
        banner: Some(include_str!("../assets/banner.svg").into()),
        config_schema: Some(vec![
            field(
                "enable_movies",
                "Films",
                "Faire tourner les affiches des films.",
                "boolean",
                "true",
            ),
            field(
                "enable_series",
                "Séries",
                "Faire tourner les affiches des séries.",
                "boolean",
                "true",
            ),
            ConfigField {
                options: vec![
                    ConfigOption {
                        label: "Aléatoire".into(),
                        value: "random".into(),
                    },
                    ConfigOption {
                        label: "Séquentiel".into(),
                        value: "sequential".into(),
                    },
                ],
                ..field(
                    "rotation_mode",
                    "Ordre de rotation",
                    "Choix du prochain visuel dans le pool.",
                    "select",
                    "random",
                )
            },
            field(
                "min_hours_between_switches",
                "Intervalle minimal (heures)",
                "0 : à chaque nouvelle visite ; 24 : une fois par jour au plus.",
                "number",
                "0",
            ),
            field(
                "pool_size",
                "Taille du pool",
                "Nombre maximal d'affiches candidates par œuvre (1 à 24).",
                "number",
                "4",
            ),
            field(
                "preferred_language",
                "Langue préférée",
                "Code de langue, par exemple fr.",
                "text",
                "fr",
            ),
            field(
                "fallback_language",
                "Langue de secours",
                "Code de langue, par exemple en.",
                "text",
                "en",
            ),
            field(
                "include_unknown_language",
                "Images sans langue",
                "Accepter les images sans langue connue.",
                "boolean",
                "true",
            ),
            field(
                "allow_any_language_fallback",
                "Autres langues en dernier recours",
                "Utiliser une autre langue si aucune image préférée n'existe.",
                "boolean",
                "true",
            ),
        ]),
        ..Manifest::default()
    })
    .unwrap_or_default()
}

pub fn handle(request: ExtensionRequest) -> ExtensionResponse {
    let result = if request.operation == "artwork.rotation.policy" {
        json!({"settings": request.input.get("config").cloned().unwrap_or_else(|| json!({}))})
    } else {
        json!({"error": "unsupported_operation"})
    };
    ExtensionResponse {
        api_version: 1,
        result,
    }
}

media_extension_sdk::extension!(handle);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_has_real_settings() {
        let m = manifest();
        assert_eq!(m["id"], "poster-rotator");
        assert!(
            m["configSchema"]
                .as_array()
                .is_some_and(|fields| fields.len() >= 8)
        );
    }
}
