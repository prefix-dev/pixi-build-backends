mod build_script;
mod config;
mod protocol;
mod python;

use protocol::PythonBuildBackendFactory;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(PythonBuildBackendFactory::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
