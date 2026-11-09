// jjhub-6js — one structured line per HTTP request: method, path,
// status, elapsed. Emitted only when a subscriber is attached (main
// wires tracing-subscriber), so the test captures one itself.
mod request_log {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn emits_one_line_per_request(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", "request-log-suite");
        let root = tempfile::tempdir().unwrap();
        let sink = Shared::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer({
                let sink = sink.clone();
                move || sink.clone()
            })
            .finish();

        let _guard = tracing::subscriber::set_default(subscriber);

        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let _ = response.into_body().collect().await.unwrap();
        drop(_guard);

        let logs = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("request"), "request line missing: {logs}");
        assert!(logs.contains("GET"), "method missing: {logs}");
        assert!(logs.contains("path=/ "), "path missing: {logs}");
        assert!(logs.contains("status=200"), "status missing: {logs}");
        assert!(logs.contains("elapsed_ms="), "elapsed missing: {logs}");
    }
}
