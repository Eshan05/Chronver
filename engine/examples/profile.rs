//! Print sanitized Markdown to help author a shared document extraction profile.
use releasescout_engine::{
    Result,
    http::{CacheOptions, Fetcher, HttpClient},
    parse,
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let url = args
        .first()
        .expect("usage: cargo run --example profile -- HTTPS_URL [CSS_ROOT]");
    let doc = HttpClient::new(CacheOptions::default())?.get(url)?;
    println!(
        "{}",
        parse::html_markdown(&doc.body, args.get(1).map(String::as_str), url)?
    );
    Ok(())
}
