use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use warp::Filter;

mod plex;

const MAX_LENGTH: u64 = 1024 * 1024;

pub struct App {
    pub cmd: String,
    pub timeout: u64,
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "plex-webhook",
    about = "Call a program with a Plex Webhook payload"
)]
pub struct Opt {
    #[structopt(
        name = "listen",
        long = "listen",
        short = "l",
        help = "address to listen on",
        default_value = "127.0.0.1"
    )]
    pub server: IpAddr,
    #[structopt(
        name = "port",
        long = "port",
        short = "p",
        help = "port to listen on",
        default_value = "8080"
    )]
    pub port: u16,
    #[structopt(
        name = "command",
        long = "command",
        short = "c",
        help = "path to the command that is execd upon each event",
        default_value = "event.sh"
    )]
    pub cmd: String,
    #[structopt(
        name = "timeout",
        long = "timeout",
        short = "t",
        help = "amount of time in seconds to allow the command to run",
        default_value = "5"
    )]
    pub timeout: u64,
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    let opt = Opt::from_args();
    let sockaddr = SocketAddr::new(opt.server, opt.port);
    let app = Arc::new(App {
        cmd: opt.cmd,
        timeout: opt.timeout,
    });
    let with_app = warp::any().map(move || app.clone());

    tracing_subscriber::fmt()
        .with_env_filter("plex_webhook=info,path::endpoint=info")
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let webhook = warp::path("plex")
        .and(warp::post())
        .and(warp::filters::multipart::form().max_length(MAX_LENGTH))
        .and(with_app)
        .and_then(plex::handle_webhook)
        .with(warp::log("path::endpoint"));

    warp::serve(webhook).run(sockaddr).await;
}
