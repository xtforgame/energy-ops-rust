use actix_web::{App, HttpServer};
use eo_platform::clock::SystemClock;
use eo_platform::config::Config;
use eo_platform::core::Core;
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // `energy-ops fake-tpr [port] [polls]`：起假的辨識服務（本機開發用）；polls ＝ 回「處理中」幾次才完成。
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("fake-tpr") {
        let port = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(18080);
        let polls = args.get(3).and_then(|p| p.parse().ok()).unwrap_or(1);
        let (url, _state, server) = eo_energy::fake_tpr::spawn(port, polls)?;
        log::info!("fake taipower-recognition listening on {url}");
        return server.await;
    }
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("config error: {e}");
        std::process::exit(2);
    });
    let addr = (config.http_host.clone(), config.http_port);
    let core = Arc::new(Core::new(config, Arc::new(SystemClock)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    }));
    let system = energy_ops::start(&core).await.unwrap_or_else(|e| {
        eprintln!("startup failed: {e}");
        std::process::exit(1);
    });
    log::info!("listening on http://{}:{}", addr.0, addr.1);
    let (s, c) = (Arc::clone(&system), Arc::clone(&core));
    let server = HttpServer::new(move || {
        App::new()
            .wrap(actix_web::middleware::Logger::new("%r %s %Dms"))
            .configure(|cfg| energy_ops::configure_app(cfg, &s, &c))
    })
    .bind(addr)?
    .run();
    let result = server.await;
    system.destroy().await;
    result
}
