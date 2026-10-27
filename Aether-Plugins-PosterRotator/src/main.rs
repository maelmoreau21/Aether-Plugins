fn main() {
    if std::env::args().nth(1).as_deref() == Some("--manifest") {
        println!("{}", aether_plugin_poster_rotator::manifest());
    } else {
        eprintln!("usage: aether-plugin-poster-rotator --manifest");
        std::process::exit(2);
    }
}
