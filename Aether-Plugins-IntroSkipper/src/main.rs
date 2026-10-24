use aether_plugin_intro_skipper::{IntroSkipperPlugin, handle_intro_skipper_request};
use std::io::Read;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plugin = IntroSkipperPlugin::new();
    match std::env::args().nth(1).as_deref() {
        Some("--manifest") => println!("{}", serde_json::to_string_pretty(&plugin.manifest())?),
        Some("--run") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            let req: media_extension_sdk::ExtensionRequest = serde_json::from_str(&buffer)?;
            let res = handle_intro_skipper_request(&plugin, req);
            println!("{}", serde_json::to_string_pretty(&res)?);
        }
        _ => {
            eprintln!(
                "usage: aether-plugin-intro-skipper --manifest | --run  (JSON request on stdin)"
            );
            std::process::exit(2);
        }
    }
    Ok(())
}
