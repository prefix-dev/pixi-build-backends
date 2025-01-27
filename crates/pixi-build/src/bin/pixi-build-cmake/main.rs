mod build_script;
mod cmake;
mod config;
mod protocol;
mod stub;

use protocol::CMakeBuildBackendInstantiator;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(CMakeBuildBackendInstantiator::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
