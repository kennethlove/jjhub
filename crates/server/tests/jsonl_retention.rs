// story: e35s01 — RED monthly JSONL rollups at the single write points.
// Expected RED: no writer exists, so no file is ever created; the drain case
// folds into the file-only assertion (drain code does not ship this story).
mod jsonl_retention {
    use axum::body::Body;
    use axum::http::{self, header, Request, StatusCode};
    use chrono::Utc;
    use std::sync::{LazyLock, Mutex};
    use tower::ServiceExt;

    const SECRET: &str = "jsonl-secret";
    static ENV: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn csrf(cookie: &str) -> String {
        server::session::csrf_token(
            SECRET.as_bytes(),
            cookie.strip_prefix("session=").unwrap_or(cookie),
        )
    }

    async fn post(
        app_: &axum::Router,
        url: &str,
        form: &str,
        cookie: &str,
    ) -> http::Response<Body> {
        app_.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(url)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from(form.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Registers, verifies the address in the database, promotes alice to
    /// operator, logs in, and returns the session cookie.
    async fn register(pool: &sqlx::PgPool, app_: &axum::Router, handle: &str) -> String {
        let email = format!("{handle}@example.test");
        let form = format!(
            "email={email}&password=correct-horse-battery&password_confirm=correct-horse-battery&_csrf="
        );
        let response = post(app_, "/register", &form, "").await;
        assert_eq!(
            response.status(),
            StatusCode::SEE_OTHER,
            "register {handle} failed"
        );
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
            .bind(&email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = 'alice'")
            .execute(pool)
            .await
            .unwrap();
        let login = post(
            app_,
            "/login",
            &format!("email={email}&password=correct-horse-battery"),
            "",
        )
        .await;
        assert_eq!(login.status(), StatusCode::SEE_OTHER, "login {handle}");
        login
            .headers()
            .get(http::header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap()
            .to_string()
    }

    fn month() -> String {
        Utc::now().format("%Y-%m").to_string()
    }

    fn lines(path: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The per-test log directory, serialised against env-touching peers and
    /// pointed at by JJHUB_LOG_DIR for the whole test.
    struct LogDir {
        _guard: std::sync::MutexGuard<'static, ()>,
        dir: tempfile::TempDir,
    }

    impl LogDir {
        fn new() -> Self {
            let guard = ENV.lock().unwrap_or_else(|err| err.into_inner());
            let dir = tempfile::tempdir().unwrap();
            std::env::set_var("SESSION_SECRET", SECRET);
            std::env::set_var("JJHUB_LOG_DIR", dir.path());
            std::env::remove_var("JJHUB_LOG_DRAIN");
            LogDir { _guard: guard, dir }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    #[sqlx::test]
    async fn one_send_appends_one_line_to_the_monthly_mail_log(pool: sqlx::PgPool) {
        let logs = LogDir::new();
        let app_ = server::app(pool.clone());
        let cookie = register(&pool, &app_, "alice").await;
        register(&pool, &app_, "bob").await;
        // The suspension notice is the one send this test performs.
        let form = format!("target=bob&_csrf={}", csrf(&cookie));
        let response = post(&app_, "/admin/suspend", &form, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let path = logs.path().join(format!("mail_log_{}.jsonl", month()));
        let lines = lines(&path);
        assert_eq!(
            lines.len(),
            1,
            "expected exactly one mail line in {}, got {:?}",
            path.display(),
            lines
        );
        assert!(
            lines[0].contains("\"to\":\"bob@example.test\""),
            "line missing the recipient: {}",
            lines[0]
        );
    }

    #[sqlx::test]
    async fn one_moderation_action_appends_one_line(pool: sqlx::PgPool) {
        let logs = LogDir::new();
        let app_ = server::app(pool.clone());
        let cookie = register(&pool, &app_, "alice").await;
        register(&pool, &app_, "bob").await;

        let form = format!("target=bob&_csrf={}&reason=for+testing", csrf(&cookie));
        let response = post(&app_, "/admin/suspend", &form, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let path = logs
            .path()
            .join(format!("moderation_log_{}.jsonl", month()));
        let lines = lines(&path);
        assert_eq!(
            lines.len(),
            1,
            "expected exactly one moderation line in {}, got {:?}",
            path.display(),
            lines
        );
        assert!(
            lines[0].contains("\"action\":\"suspend\""),
            "line missing the action: {}",
            lines[0]
        );
    }

    #[sqlx::test]
    async fn every_line_is_a_json_object_with_ts_first(pool: sqlx::PgPool) {
        let logs = LogDir::new();
        let app_ = server::app(pool.clone());
        let cookie = register(&pool, &app_, "alice").await;
        register(&pool, &app_, "bob").await;

        let form = format!("target=bob&_csrf={}", csrf(&cookie));
        let response = post(&app_, "/admin/suspend", &form, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let path = logs
            .path()
            .join(format!("moderation_log_{}.jsonl", month()));
        let lines = lines(&path);
        assert_eq!(lines.len(), 1, "one line per event");
        let line = &lines[0];
        assert!(
            line.starts_with("{\"ts\":\""),
            "ts must be the first field: {line}"
        );
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|err| panic!("line is not one JSON object: {err}: {line}"));
        chrono::DateTime::parse_from_rfc3339(value["ts"].as_str().unwrap()).expect("ts is rfc3339");
        assert_eq!(value["action"], "suspend");
    }

    #[sqlx::test]
    async fn events_in_one_month_share_one_file_named_for_its_write_date(pool: sqlx::PgPool) {
        let logs = LogDir::new();
        let app_ = server::app(pool.clone());
        let cookie = register(&pool, &app_, "alice").await;
        register(&pool, &app_, "bob").await;

        for action in ["/admin/suspend", "/admin/unsuspend"] {
            let form = format!("target=bob&_csrf={}", csrf(&cookie));
            let response = post(&app_, action, &form, &cookie).await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{action}");
        }

        let path = logs
            .path()
            .join(format!("moderation_log_{}.jsonl", month()));
        assert!(
            path.exists(),
            "rotation follows the write date, not a timer: {}",
            path.display()
        );
        assert_eq!(lines(&path).len(), 2, "two events share the month file");
    }

    #[sqlx::test]
    async fn an_unset_drain_env_writes_only_the_files(pool: sqlx::PgPool) {
        let logs = LogDir::new();
        std::env::remove_var("JJHUB_LOG_DRAIN");
        let app_ = server::app(pool.clone());
        let cookie = register(&pool, &app_, "alice").await;
        register(&pool, &app_, "bob").await;

        let form = format!("target=bob&_csrf={}", csrf(&cookie));
        let response = post(&app_, "/admin/suspend", &form, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let mut entries: Vec<String> = std::fs::read_dir(logs.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        assert_eq!(
            entries,
            vec![
                format!("mail_log_{}.jsonl", month()),
                format!("moderation_log_{}.jsonl", month()),
            ],
            "no drain side effects: only the two rollup files"
        );
    }
}
