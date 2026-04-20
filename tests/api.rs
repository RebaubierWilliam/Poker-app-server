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

#[tokio::test]
async fn get_malette_by_id_returns_stored_row() {
    let (app, _pool) = common::test_app().await;

    let body = json!({
        "name": "M1",
        "chips": [{"value": 25, "count": 100}]
    });
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: serde_json::Value = read_json(created).await;
    let id = created["id"].as_i64().unwrap();

    let res = with_api_key(app, "GET", &format!("/malettes/{id}"), None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["id"], id);
    assert_eq!(got["name"], "M1");
}

#[tokio::test]
async fn get_malette_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let res = with_api_key(app, "GET", "/malettes/9999", None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_malettes_returns_all_rows_ordered_by_id() {
    let (app, _pool) = common::test_app().await;

    for name in ["A", "B", "C"] {
        let body = json!({"name": name, "chips": [{"value": 25, "count": 10}]});
        let r = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
        assert_eq!(r.status(), StatusCode::CREATED);
    }

    let res = with_api_key(app, "GET", "/malettes", None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list: serde_json::Value = read_json(res).await;
    let arr = list.as_array().expect("array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["name"], "A");
    assert_eq!(arr[2]["name"], "C");
}

#[tokio::test]
async fn put_malette_updates_row_and_bumps_updated_at() {
    let (app, _pool) = common::test_app().await;

    let body = json!({"name": "old", "chips": [{"value": 25, "count": 10}]});
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
    let created: serde_json::Value = read_json(created).await;
    let id = created["id"].as_i64().unwrap();
    let old_updated = created["updated_at"].as_str().unwrap().to_string();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let new_body = json!({"name": "new", "chips": [{"value": 100, "count": 50}]});
    let res = with_api_key(app.clone(), "PUT", &format!("/malettes/{id}"), Some(new_body)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["name"], "new");
    assert_eq!(got["chips"][0]["value"], 100);
    assert_ne!(got["updated_at"].as_str().unwrap(), old_updated);
}

#[tokio::test]
async fn put_malette_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let body = json!({"name": "x", "chips": [{"value": 25, "count": 10}]});
    let res = with_api_key(app, "PUT", "/malettes/9999", Some(body)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
