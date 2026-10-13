//! Loopback HTTP serving for an immutable architecture snapshot.
use axum::{Router, http::header, response::Html, routing::get};
use std::sync::Arc;

pub fn serve(port: u16, html: String) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        let html: Arc<str> = html.into();
        let page = get(move || {
            let html = html.clone();
            async move {
                (
                    [(header::CACHE_CONTROL, "no-store")],
                    Html(html.to_string()),
                )
            }
        });
        let app = Router::new()
            .route("/", page.clone())
            .route("/sage-browser.html", page);
        println!(
            "Sage architecture browser: http://{}/",
            listener.local_addr()?
        );
        println!("Snapshot generated at startup. Restart after edits. Press Ctrl-C to stop.");
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                if let Err(error) = tokio::signal::ctrl_c().await {
                    eprintln!("Cannot listen for Ctrl-C: {error}");
                }
            })
            .await
    })?;
    Ok(())
}
