mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

pub async fn with_api_key(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-api-key", common::TEST_API_KEY);
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let body = match body {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    app.oneshot(req.body(body).unwrap()).await.unwrap()
}

pub async fn read_json(res: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_is_public_and_returns_ok() {
    let (app, _pool) = common::test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn post_malettes_creates_and_returns_the_row() {
    let (app, _pool) = common::test_app().await;
    let body = json!({
        "name": "Starter 300",
        "chips": [
            {"value": 25, "count": 100},
            {"value": 100, "count": 80}
        ]
    });
    let res = with_api_key(app, "POST", "/malettes", Some(body.clone())).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert!(res.headers().contains_key("location"));
    let got: serde_json::Value = read_json(res).await;
    assert!(got["id"].is_number());
    assert_eq!(got["name"], "Starter 300");
    assert_eq!(got["chips"], body["chips"]);
    assert!(got["created_at"].is_string());
    assert!(got["updated_at"].is_string());
}
