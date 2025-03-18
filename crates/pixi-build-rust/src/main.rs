mod build_script;
mod config;
mod protocol;
mod rust;

use protocol::RustBackendInstantiator;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(RustBackendInstantiator::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
