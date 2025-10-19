#[macro_use]
extern crate log;

mod model;
mod dex;
mod error;
mod trader;
mod request;

use anyhow::{Result};

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    println!("Hello, world!");
    Ok(())
}
