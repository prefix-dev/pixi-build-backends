mod protocol;
mod rattler_build;

use protocol::RattlerBuildBackendFactory;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(RattlerBuildBackendFactory::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
