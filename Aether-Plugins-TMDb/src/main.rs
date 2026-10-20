use aether_plugin_api::{
    DetailsRequest, ImagesRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
    handle_request,
};
use aether_plugin_tmdb::TmdbPlugin;
use clap::{Parser, Subcommand};
use std::io::{self, Read};

#[derive(Parser)]
#[command(
    name = "aether-plugin-tmdb",
    author = "Aether Team",
    version = "0.1.0",
    about = "TMDb Metadata Provider Extension for Aether"
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
    /// Search TMDb for movies or TV shows
    Search {
        #[arg(short, long)]
        title: String,

        #[arg(short, long)]
        year: Option<u16>,

        #[arg(short, long, default_value = "movie")]
        media_type: String,

        #[arg(short, long)]
        api_key: Option<String>,

        #[arg(short, long, default_value = "fr-FR")]
        language: String,
    },
    /// Fetch complete details for a TMDb ID
    Details {
        #[arg(short, long)]
        id: String,

        #[arg(short, long, default_value = "movie")]
        media_type: String,

        #[arg(short, long)]
        api_key: Option<String>,

        #[arg(short, long, default_value = "fr-FR")]
        language: String,
    },
    /// Fetch artwork for a TMDb ID
    Images {
        #[arg(short, long)]
        id: String,

        #[arg(short, long, default_value = "movie")]
        media_type: String,

        #[arg(short, long)]
        api_key: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let plugin = TmdbPlugin::new();

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
        Some(Commands::Search {
            title,
            year,
            media_type,
            api_key,
            language,
        }) => {
            let m_type = match media_type.to_lowercase().as_str() {
                "tv" | "tvshow" | "series" => MediaType::TvShow,
                "anime" => MediaType::Anime,
                _ => MediaType::Movie,
            };
            let config = ProviderConfig {
                api_key,
                language: Some(language.clone()),
                ..Default::default()
            };
            let results = plugin
                .search(
                    SearchQuery {
                        title,
                        year,
                        media_type: m_type,
                        language: Some(language),
                        external_ids: Default::default(),
                    },
                    &config,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        Some(Commands::Details {
            id,
            media_type,
            api_key,
            language,
        }) => {
            let m_type = match media_type.to_lowercase().as_str() {
                "tv" | "tvshow" | "series" => MediaType::TvShow,
                _ => MediaType::Movie,
            };
            let config = ProviderConfig {
                api_key,
                language: Some(language),
                ..Default::default()
            };
            let details = plugin
                .details(
                    DetailsRequest {
                        id,
                        media_type: m_type,
                        external_ids: Default::default(),
                    },
                    &config,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&details)?);
        }
        Some(Commands::Images {
            id,
            media_type,
            api_key,
        }) => {
            let m_type = match media_type.to_lowercase().as_str() {
                "tv" | "tvshow" | "series" => MediaType::TvShow,
                _ => MediaType::Movie,
            };
            let config = ProviderConfig {
                api_key,
                ..Default::default()
            };
            let images = plugin
                .images(
                    ImagesRequest {
                        id,
                        media_type: m_type,
                        external_ids: Default::default(),
                    },
                    &config,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&images)?);
        }
        None => {
            let manifest = plugin.manifest();
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
    }

    Ok(())
}
