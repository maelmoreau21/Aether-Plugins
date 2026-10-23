use aether_plugin_api::{MetadataProviderPlugin, handle_request};
use aether_plugin_omdb::OmdbPlugin;
use std::io::Read;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugin = OmdbPlugin::new();
    match std::env::args().nth(1).as_deref() {
        Some("--manifest") => println!("{}", serde_json::to_string_pretty(&plugin.manifest())?),
        Some("--run") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            println!("{}", handle_request(&plugin, &buffer).await);
        }
        _ => {
            eprintln!("usage: aether-plugin-omdb --manifest | --run  (JSON request on stdin)");
            std::process::exit(2);
        }
    }
    Ok(())
}
