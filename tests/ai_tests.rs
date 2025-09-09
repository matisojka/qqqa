use httpmock::Method::POST;
use httpmock::{MockServer};
use qqqa::ai::ChatClient;
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
