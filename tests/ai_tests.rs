use httpmock::Method::POST;
use httpmock::MockServer;
use qqqa::ai::ChatClient;
use serde_json::Value;
use std::net::TcpListener;

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
async fn gpt5_mini_uses_max_completion_tokens() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .json_body_partial(r#"{"model":"gpt-5-mini"}"#)
            .matches(|req| {
                // Verify that the body contains max_completion_tokens and NOT max_tokens
                // Also verify that temperature is NOT included (gpt-5 doesn't support it)
                if let Some(body_bytes) = &req.body {
                    if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                        return body.get("max_completion_tokens").is_some()
                            && body.get("max_tokens").is_none()
                            && body.get("temperature").is_none();
                    }
                }
                false
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"GPT-5 response"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("gpt-5-mini", "Hi", true).await.unwrap();
    assert_eq!(got, "GPT-5 response");
    mock.assert();
}

#[tokio::test]
async fn gpt4_uses_max_tokens() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .json_body_partial(r#"{"model":"gpt-4"}"#)
            .matches(|req| {
                // Verify that the body contains max_tokens and NOT max_completion_tokens
                // Also verify that temperature IS included (older models support it)
                if let Some(body_bytes) = &req.body {
                    if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                        return body.get("max_tokens").is_some()
                            && body.get("max_completion_tokens").is_none()
                            && body.get("temperature").is_some();
                    }
                }
                false
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"GPT-4 response"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("gpt-4", "Hi", true).await.unwrap();
    assert_eq!(got, "GPT-4 response");
    mock.assert();
}

#[tokio::test]
async fn o1_preview_uses_max_completion_tokens() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .json_body_partial(r#"{"model":"o1-preview"}"#)
            .matches(|req| {
                // Verify that the body contains max_completion_tokens and NOT max_tokens
                // Also verify that temperature is NOT included (gpt-5 doesn't support it)
                if let Some(body_bytes) = &req.body {
                    if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                        return body.get("max_completion_tokens").is_some()
                            && body.get("max_tokens").is_none()
                            && body.get("temperature").is_none();
                    }
                }
                false
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"O1 response"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("o1-preview", "Hi", true).await.unwrap();
    assert_eq!(got, "O1 response");
    mock.assert();
}

#[tokio::test]
async fn o3_mini_uses_max_completion_tokens() {
    if sandbox_blocks_binding() {
        eprintln!("[skip] sandbox blocks binding to 127.0.0.1; skipping httpmock test");
        return;
    }
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .json_body_partial(r#"{"model":"o3-mini"}"#)
            .matches(|req| {
                // Verify that the body contains max_completion_tokens and NOT max_tokens
                // Also verify that temperature is NOT included (gpt-5 doesn't support it)
                if let Some(body_bytes) = &req.body {
                    if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                        return body.get("max_completion_tokens").is_some()
                            && body.get("max_tokens").is_none()
                            && body.get("temperature").is_none();
                    }
                }
                false
            });
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"choices":[{"message":{"content":"O3 response"}}]}"#);
    });

    let client = ChatClient::new(server.base_url(), "test".into()).unwrap();
    let got = client.chat_once("o3-mini", "Hi", true).await.unwrap();
    assert_eq!(got, "O3 response");
    mock.assert();
}
