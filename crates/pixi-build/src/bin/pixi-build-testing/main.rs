mod config;
mod protocol;
mod testing;

use protocol::TestingBackendInstantiator;

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(TestingBackendInstantiator::new).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
