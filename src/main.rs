mod api;
mod bot;

use api::run_api;
use bot::run_bot;

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");

    tokio::select! {
        _ = run_bot(token) => {},
        _ = run_api() => {},
    }
}