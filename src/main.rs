use actix_web::{App, HttpServer};
use eo_platform::clock::SystemClock;
use eo_platform::config::Config;
use eo_platform::core::Core;
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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
