use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--manifest") => println!("{}", aether_plugin_requests::manifest()),
        Some("--run") => {
            let mut input = String::new();
            std::io::stdin().take(65_537).read_to_string(&mut input)?;
            if input.len() > 65_536 {
                return Err("input too large".into());
            }
            let request = serde_json::from_str(&input)?;
            println!(
                "{}",
                serde_json::to_string(&aether_plugin_requests::handle_extension(request))?
            );
        }
        _ => {
            eprintln!("usage: aether-plugin-requests --manifest | --run");
            std::process::exit(2);
        }
    }
    Ok(())
}
