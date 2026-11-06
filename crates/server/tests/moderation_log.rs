// story: e33s01

mod moderation_log {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn register(pool: &sqlx::PgPool, local: &str) -> String {
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let response = server::app(pool.clone())
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery\
                         &password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        let response = server::app(pool.clone())
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        cookie_of(&response)
    }

    fn cookie_of(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn make_operator(pool: &sqlx::PgPool, local: &str) -> String {
        let cookie = register(pool, local).await;
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        cookie
    }

    async fn get(
        pool: &sqlx::PgPool,
        path: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        server::app(pool.clone()).oneshot(request).await.unwrap()
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    /// The log section starts at its heading; slicing there keeps
    /// account-list handles from satisfying log assertions.
    fn log_section(body: &str) -> &str {
        // e36s01: the console gained a deliveries panel after this one —
        // slice to the section's close so later panels stay out.
        let Some(start) = body.find("Moderation log") else {
            return body;
        };
        let rest = &body[start..];
        match rest.find("</section>") {
            Some(end) => &rest[..end],
            None => rest,
        }
    }

    async fn seed_row(
        pool: &sqlx::PgPool,
        actor: &str,
        target: Option<&str>,
        action: &str,
        at: &str,
    ) {
        sqlx::query(
            "INSERT INTO moderation_actions (actor_id, target_id, action, created_at)
             SELECT a.id, t.id, $3, $4::timestamptz
             FROM users a LEFT JOIN users t ON t.handle = $2
             WHERE a.handle = $1",
        )
        .bind(actor)
        .bind(target)
        .bind(action)
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn log_lists_rows_newest_first_for_operator(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        register(&pool, "bob").await;
        register(&pool, "carol").await;
        seed_row(
            &pool,
            "alice",
            Some("bob"),
            "suspend",
            "2026-11-01T10:00:00Z",
        )
        .await;
        seed_row(
            &pool,
            "alice",
            Some("carol"),
            "unsuspend",
            "2026-11-02T10:00:00Z",
        )
        .await;

        let response = get(&pool, "/admin", Some(&alice)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let section = log_section(&body_of(response).await).to_string();
        assert!(section.contains("Moderation log"), "log section present");
        assert!(section.contains("bob"), "target listed");
        assert!(section.contains("carol"), "second target listed");
        assert!(section.contains("alice"), "actor listed");
        let newer = section.find("carol").expect("carol row");
        let older = section.find("bob").expect("bob row");
        assert!(newer < older, "newest-first order: {section}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn action_filter_limits_the_list(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        register(&pool, "bob").await;
        register(&pool, "carol").await;
        seed_row(
            &pool,
            "alice",
            Some("bob"),
            "suspend",
            "2026-11-01T10:00:00Z",
        )
        .await;
        seed_row(
            &pool,
            "alice",
            None,
            "registration_close",
            "2026-11-02T10:00:00Z",
        )
        .await;

        let response = get(&pool, "/admin?action=registration_close", Some(&alice)).await;
        let section = log_section(&body_of(response).await).to_string();
        assert!(
            section.contains("registration_close"),
            "close row shown: {section}"
        );
        assert!(!section.contains("bob"), "suspend row filtered out");

        let response = get(&pool, "/admin?action=suspend", Some(&alice)).await;
        let section = log_section(&body_of(response).await).to_string();
        assert!(section.contains("bob"), "suspend row shown");
        assert!(
            !section.contains("registration_close"),
            "close row filtered out"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn target_filter_limits_the_list(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        register(&pool, "bob").await;
        register(&pool, "carol").await;
        seed_row(
            &pool,
            "alice",
            Some("bob"),
            "suspend",
            "2026-11-01T10:00:00Z",
        )
        .await;
        seed_row(
            &pool,
            "alice",
            Some("carol"),
            "suspend",
            "2026-11-02T10:00:00Z",
        )
        .await;

        let response = get(&pool, "/admin?target=bob", Some(&alice)).await;
        let section = log_section(&body_of(response).await).to_string();
        assert!(section.contains("bob"), "bob's row shown");
        assert!(!section.contains("carol"), "carol's row filtered out");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn csv_export_has_header_and_quoted_fields(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        register(&pool, "bob").await;
        register(&pool, "carol").await;
        seed_row(
            &pool,
            "alice",
            Some("bob"),
            "suspend",
            "2026-11-01T10:00:00Z",
        )
        .await;
        seed_row(
            &pool,
            "alice",
            Some("carol"),
            "unsuspend",
            "2026-11-02T10:00:00Z",
        )
        .await;

        let response = get(&pool, "/admin/moderation.csv", Some(&alice)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            content_type.starts_with("text/csv"),
            "csv content type: {content_type}"
        );
        let disposition = response
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            disposition.contains("attachment"),
            "downloads as a file: {disposition}"
        );
        let body = body_of(response).await;
        assert!(
            body.starts_with("actor,target,action,created_at"),
            "header row: {body}"
        );
        assert!(body.contains("\"alice\",\"bob\",\"suspend\""), "quoted row");
        let csv = get(&pool, "/admin/moderation.csv?target=carol", Some(&alice)).await;
        let filtered = body_of(csv).await;
        assert!(!filtered.contains("bob"), "export honors filters");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_operator_gets_404_everywhere(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        let bob = register(&pool, "bob").await;
        seed_row(
            &pool,
            "alice",
            Some("bob"),
            "suspend",
            "2026-11-01T10:00:00Z",
        )
        .await;
        drop(alice);

        let response = get(&pool, "/admin/moderation.csv", Some(&bob)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = body_of(response).await;
        assert!(!body.contains("bob,\""), "no rows leak through: {body}");

        let response = get(&pool, "/admin?action=suspend", Some(&bob)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn empty_log_renders_an_explicit_empty_state(pool: sqlx::PgPool) {
        let alice = make_operator(&pool, "alice").await;
        let response = get(&pool, "/admin", Some(&alice)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let section = log_section(&body_of(response).await).to_string();
        assert!(
            section.contains("No moderation actions yet"),
            "explicit empty state: {section}"
        );
    }
}
