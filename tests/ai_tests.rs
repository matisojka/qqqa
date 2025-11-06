use httpmock::Method::POST;
use httpmock::MockServer;
use httpmock::prelude::HttpMockRequest;
use qqqa::ai::ChatClient;
use std::net::TcpListener;
use serde_json::Value;

fn sandbox_blocks_binding() -> bool {
    TcpListener::bind("127.0.0.1:0").is_err()
}

#[tokio::test]
async fn chat_once_non_streaming_parses_response() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"Hello world"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("model-x", "Hi", true).await.unwrap();
    assert_eq!(got, "Hello world");
    mock.assert();
}

#[tokio::test]
async fn chat_stream_streams_tokens() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"He\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let mock = server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body(sse_body);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let mut acc = String::new();
    client
        .chat_stream("model-x", "Hi", true, |tok| acc.push_str(tok))
        .await
        .unwrap();
    assert_eq!(acc, "Hello");
    mock.assert();
}

#[tokio::test]
async fn chat_once_uses_new_parameters_for_new_models() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .matches(|req: &HttpMockRequest| {
                let body = req
                    .body
                    .as_ref()
                    .expect("expected request body for new model");
                let payload: Value = serde_json::from_slice(body)
                    .expect("request body should be valid JSON");
                assert!(payload.get("max_completion_tokens").is_some());
                assert!(payload.get("max_tokens").is_none());
                assert!(payload.get("temperature").is_none());
                true
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"ok"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("gpt-5-mini", "Hi", false).await.unwrap();
    assert_eq!(got, "ok");
    mock.assert();
}

#[tokio::test]
async fn chat_once_uses_legacy_parameters_for_old_models() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .matches(|req: &HttpMockRequest| {
                let body = req
                    .body
                    .as_ref()
                    .expect("expected request body for legacy model");
                let payload: Value = serde_json::from_slice(body)
                    .expect("request body should be valid JSON");
                assert_eq!(payload.get("max_tokens").and_then(|v| v.as_u64()), Some(4000));
                assert!(payload.get("max_completion_tokens").is_none());
                assert_eq!(payload.get("temperature").and_then(|v| v.as_f64()), Some(0.0));
                true
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"ok"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("gpt-4.1-mini", "Hi", false).await.unwrap();
    assert_eq!(got, "ok");
    mock.assert();
}
