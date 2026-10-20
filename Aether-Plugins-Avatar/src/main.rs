use aether_plugin_api::{MetadataProviderPlugin, handle_request};
use aether_plugin_avatar::AvatarPlugin;
use clap::{Parser, Subcommand};
use std::io::{self, Read};

#[derive(Parser)]
#[command(
    name = "aether-plugin-avatar",
    author = "Aether Team",
    version = "0.1.0",
    about = "Avatar packs and profile picture management extension for Aether"
)]
struct Cli {
    /// Print plugin manifest JSON and exit
    #[arg(long)]
    manifest: bool,

    /// Execute standard IPC loop: reads single JSON request from STDIN and writes JSON to STDOUT
    #[arg(long)]
    run: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List available avatar packs
    Packs,
    /// Get avatars from a specific pack
    Avatars {
        #[arg(short, long, default_value = "personas")]
        pack: String,
    },
    /// Pick a random avatar from a pack (optional seed)
    Random {
        #[arg(short, long, default_value = "personas")]
        pack: String,

        #[arg(short, long)]
        seed: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let plugin = AvatarPlugin::new();

    if cli.manifest {
        let manifest = plugin.manifest();
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    if cli.run {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        let response = handle_request(&plugin, &buffer).await;
        println!("{response}");
        return Ok(());
    }

    match cli.command {
        Some(Commands::Packs) => {
            let packs = plugin.packs();
            println!("{}", serde_json::to_string_pretty(&packs)?);
        }
        Some(Commands::Avatars { pack }) => {
            let avatars = plugin.get_pack_avatars(&pack);
            println!("{}", serde_json::to_string_pretty(&avatars)?);
        }
        Some(Commands::Random { pack, seed }) => {
            let avatar = plugin.random_avatar(Some(&pack), seed.as_deref());
            println!("{}", serde_json::to_string_pretty(&avatar)?);
        }
        None => {
            let manifest = plugin.manifest();
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
    }

    Ok(())
}
