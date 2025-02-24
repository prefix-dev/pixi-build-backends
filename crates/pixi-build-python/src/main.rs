mod build_script;
mod config;
mod protocol;
mod python;

use protocol::PythonBuildBackendInstantiator;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(PythonBuildBackendInstantiator::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
