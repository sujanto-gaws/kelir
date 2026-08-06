mod config;
mod db;
mod error;
mod health;
mod middleware;
mod modules;
mod router;
mod utils;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    println!("Kelir backend skeleton is ready");
}
