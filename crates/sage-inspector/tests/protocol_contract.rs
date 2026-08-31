use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use sage_inspector::scripted::ScriptedProvider;
use sage_inspector::{InspectionClient, serve_actor};
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Deserialize)]
struct RouteFixture {
    request: FixtureRequest,
    response: FixtureResponse,
    expected_demand: String,
}

#[derive(Deserialize)]
struct FixtureRequest {
    method: String,
    path: String,
    headers: Vec<HeaderEntry>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct FixtureResponse {
    status: u16,
    headers: Vec<HeaderEntry>,
    body: String,
}

#[derive(Deserialize)]
struct HeaderEntry {
    name: String,
    value: String,
}

#[tokio::test]
async fn scripted_axum_bytes_match_the_reviewed_protocol_fixture() {
    let (client, actor) = InspectionClient::channel();
    let (demand_sender, demand_receiver) = std::sync::mpsc::channel();
    let mut actor = actor;
    actor.observe_demands(demand_sender);
    let actor_thread = std::thread::spawn(move || serve_actor(ScriptedProvider::default(), actor));
    let app = sage_inspector::server::router(client.clone());
    let routes: Vec<RouteFixture> =
        serde_json::from_str(&std::fs::read_to_string(fixture_root().join("routes.json")).unwrap())
            .unwrap();
    assert_eq!(
        routes.len(),
        25,
        "the manifest covers every fixture resource"
    );
    for route in routes {
        assert_eq!(route.request.method, "GET");
        assert!(route.request.body.is_none());
        assert_eq!(route.request.headers.len(), 1);
        assert_eq!(route.request.headers[0].name, "accept");
        assert_eq!(route.request.headers[0].value, "application/json");
        assert_eq!(route.response.status, 200);
        assert_eq!(route.response.headers.len(), 2);
        assert!(fixture_root().join(&route.expected_demand).is_file());
        assert_route(
            app.clone(),
            &route.request.path,
            route.response.body.strip_prefix("responses/").unwrap(),
        )
        .await;
        let actual_demand = demand_receiver.recv().unwrap().fixture_line();
        let expected_demand =
            std::fs::read_to_string(fixture_root().join(&route.expected_demand)).unwrap();
        let mut expected_lines = expected_demand.lines();
        assert_eq!(Some(actual_demand.as_str()), expected_lines.next());
        for forbidden in expected_lines {
            let forbidden = forbidden
                .strip_prefix("forbidden: ")
                .expect("additional demand lines are negative assertions");
            assert!(
                !actual_demand.contains(forbidden),
                "observed forbidden demand `{forbidden}`"
            );
        }
        assert!(
            matches!(
                demand_receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ),
            "one HTTP resource must cause exactly one provider operation"
        );
    }

    drop(app);
    drop(client);
    actor_thread.join().unwrap();
}

#[tokio::test]
async fn every_advertised_scripted_product_is_fetchable() {
    let (client, actor) = InspectionClient::channel();
    let actor_thread = std::thread::spawn(move || serve_actor(ScriptedProvider::default(), actor));
    let app = sage_inspector::server::router(client.clone());

    let (status, index) = request_json(app.clone(), "/api/v1/symbols").await;
    assert_eq!(status, StatusCode::OK);
    let symbols = index["value"]["symbols"]
        .as_array()
        .expect("the symbol index contains a symbol array");
    let mut failures = Vec::new();

    for summary in symbols {
        let path = summary["path"]
            .as_str()
            .expect("every symbol summary has a path");
        let encoded_path = utf8_percent_encode(path, NON_ALPHANUMERIC);
        let (status, selected) =
            request_json(app.clone(), &format!("/api/v1/symbol?path={encoded_path}")).await;
        if status != StatusCode::OK {
            failures.push(format!("selecting `{path}` returned {status}: {selected}"));
            continue;
        }
        if selected["value"]["path"].as_str() != Some(path) {
            failures.push(format!(
                "selecting `{path}` returned symbol path `{}`",
                selected["value"]["path"]
            ));
            continue;
        }

        let products = selected["value"]["products"]
            .as_array()
            .expect("every selected symbol has a product array");
        for descriptor in products {
            let id = descriptor["id"]
                .as_str()
                .expect("every product descriptor has an id");
            let href = descriptor["href"]
                .as_str()
                .expect("every product descriptor has an href");
            let (status, page) = request_json(app.clone(), href).await;
            if status != StatusCode::OK {
                failures.push(format!(
                    "advertised product `{id}` for `{path}` returned {status}: {page}"
                ));
                continue;
            }
            if page["value"]["id"] != id {
                failures.push(format!(
                    "advertised product `{id}` for `{path}` returned page id `{}`",
                    page["value"]["id"]
                ));
            }
        }
    }

    drop(app);
    drop(client);
    actor_thread.join().unwrap();
    assert!(
        failures.is_empty(),
        "every advertised fixture product must be fetchable:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
async fn missing_semantic_resources_are_revision_tagged_structured_errors() {
    let (client, actor) = InspectionClient::channel();
    let actor_thread = std::thread::spawn(move || serve_actor(ScriptedProvider::default(), actor));
    let app = sage_inspector::server::router(client.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/symbol?path=local%2Fmissing")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["revision_id"], "rev_0");
    assert_eq!(error["run_id"], serde_json::Value::Null);
    assert_eq!(error["error"]["code"], "symbol-not-found");

    let malformed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/product?symbol=local%2Fdb-drop-guard")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let body = malformed.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["revision_id"], "rev_0");
    assert_eq!(error["error"]["code"], "invalid-request");

    let unknown = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/not-a-route")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let body = unknown.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["revision_id"], "rev_0");
    assert_eq!(error["error"]["code"], "not-found");

    drop(client);
    actor_thread.join().unwrap();
}

#[tokio::test]
async fn spa_routes_use_html_but_missing_assets_do_not_fall_back() {
    let (client, actor) = InspectionClient::channel();
    let actor_thread = std::thread::spawn(move || serve_actor(ScriptedProvider::default(), actor));
    let app = sage_inspector::server::router(client.clone());

    let navigation = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/symbols/local%2Fdb-drop-guard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(navigation.status(), StatusCode::OK);
    assert_eq!(navigation.headers()["content-type"], "text/html");

    let missing_asset = app
        .oneshot(
            Request::builder()
                .uri("/assets/missing.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_asset.status(), StatusCode::NOT_FOUND);

    drop(client);
    actor_thread.join().unwrap();
}

async fn assert_route(app: axum::Router, uri: &str, fixture: &str) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/json; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let actual = String::from_utf8(body.to_vec()).unwrap();
    let expected = fixture_path(fixture);
    let expected = std::fs::read_to_string(expected).unwrap();
    snapbox::Assert::new().eq(
        snapbox::Data::text(actual),
        snapbox::Data::text(expected).raw(),
    );
}

async fn request_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&body).unwrap();
    (status, value)
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    fixture_root().join("responses").join(name)
}

fn fixture_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/semantic-inspector/db-drop-guard/api")
}
