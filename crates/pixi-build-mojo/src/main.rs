mod build_script;
mod config;
mod mojo;
mod protocol;

use protocol::MojoBackendInstantiator;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(MojoBackendInstantiator::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
