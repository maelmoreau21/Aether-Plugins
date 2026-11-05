use aether_plugin_api::MetadataProviderPlugin;
use aether_plugin_tvdb::TvdbPlugin;
use std::io::Read;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--manifest") => println!("{}", serde_json::to_string_pretty(&TvdbPlugin.manifest())?),
        Some("--run") => {
            let mut input = String::new();
            std::io::stdin().take(65_537).read_to_string(&mut input)?;
            if input.len() > 65_536 {
                return Err("input too large".into());
            }
            println!(
                "{}",
                aether_plugin_api::handle_request(&TvdbPlugin, &input).await
            );
        }
        _ => {
            eprintln!("usage: aether-plugin-tvdb --manifest | --run");
            std::process::exit(2);
        }
    }
    Ok(())
}
