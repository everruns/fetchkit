//! Fetchkit CLI binary. All logic lives in the library (`fetchkit_cli::run`)
//! so embedders such as bashkit expose the exact same interface.

#[tokio::main]
async fn main() {
    let code = fetchkit_cli::run(std::env::args_os(), &fetchkit_cli::NativeHost).await;
    std::process::exit(code);
}
