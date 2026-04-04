//! Integration tests for `FeedThread` with a local WebSocket echo server.

#![expect(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use mantis_transport::{BackoffConfig, FeedConfig, FeedThread, SocketTuning, WsConfig};

async fn start_echo_server() -> (String, tokio::sync::oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        tokio::spawn(async move {
                            let ws = accept_async(stream).await.unwrap();
                            let (mut write, mut read) = ws.split();
                            while let Some(Ok(msg)) = read.next().await {
                                match msg {
                                    Message::Text(text) if text.as_str() == "PING" => {
                                        let _ = write.send(Message::Text("PONG".into())).await;
                                    }
                                    Message::Text(text) => {
                                        let _ = write.send(Message::Text(text)).await;
                                    }
                                    Message::Ping(data) => {
                                        let _ = write.send(Message::Pong(data)).await;
                                    }
                                    Message::Close(_) => break,
                                    _ => {}
                                }
                            }
                        });
                    }
                }
            }
        }
    });

    (format!("ws://{addr}"), shutdown_tx)
}

async fn start_counting_server(count: u32) -> (String, tokio::sync::oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        tokio::spawn(async move {
                            let ws = accept_async(stream).await.unwrap();
                            let (mut write, mut read) = ws.split();
                            let drain = tokio::spawn(async move {
                                while let Some(Ok(_)) = read.next().await {}
                            });
                            for i in 0..count {
                                let msg = format!(r#"{{"event_type":"test","seq":{i}}}"#);
                                if write.send(Message::Text(msg.into())).await.is_err() {
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                            let _ = write.send(Message::Close(None)).await;
                            drain.abort();
                        });
                    }
                }
            }
        }
    });

    (format!("ws://{addr}"), shutdown_tx)
}

fn make_config(url: String) -> FeedConfig {
    FeedConfig {
        name: "test-feed".to_owned(),
        ws: WsConfig {
            url,
            subscribe_msg: None,
            ping_interval: None,
            read_timeout: Some(Duration::from_secs(5)),
        },
        tuning: SocketTuning::default(),
        backoff: BackoffConfig::default(),
    }
}

#[tokio::test]
async fn feed_thread_receives_messages() {
    let (url, _shutdown) = start_counting_server(20).await;
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = make_config(url);
    let handle = FeedThread::spawn(config, move |_msg| {
        r.fetch_add(1, Ordering::Relaxed);
        true
    })
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    let count = received.load(Ordering::Relaxed);
    assert!(count >= 10, "expected >= 10 messages, got {count}");
    assert_eq!(handle.msg_count.load(Ordering::Relaxed), u64::from(count));
    handle.shutdown();
}

#[tokio::test]
async fn feed_thread_shutdown_graceful() {
    let (url, _shutdown) = start_echo_server().await;
    let mut config = make_config(url);
    config.ws.read_timeout = Some(Duration::from_secs(1));

    let handle = FeedThread::spawn(config, |_msg| true).unwrap();
    assert!(handle.is_running());
    handle.shutdown();
}

#[tokio::test]
async fn feed_thread_callback_stop() {
    let (url, _shutdown) = start_counting_server(100).await;
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = make_config(url);
    let handle = FeedThread::spawn(config, move |_msg| {
        let n = r.fetch_add(1, Ordering::Relaxed);
        n < 4
    })
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    let count = received.load(Ordering::Relaxed);
    assert_eq!(count, 5, "expected exactly 5 messages, got {count}");
    assert!(!handle.is_running());
    handle.shutdown();
}

#[tokio::test]
async fn feed_thread_reconnects_on_server_close() {
    let (url, _shutdown) = start_counting_server(3).await;
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = FeedConfig {
        name: "reconnect-test".to_owned(),
        ws: WsConfig {
            url,
            subscribe_msg: None,
            ping_interval: None,
            read_timeout: Some(Duration::from_secs(2)),
        },
        tuning: SocketTuning::default(),
        backoff: BackoffConfig {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(500),
            jitter_factor: 0.0,
        },
    };

    let handle = FeedThread::spawn(config, move |_msg| {
        r.fetch_add(1, Ordering::Relaxed);
        true
    })
    .unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;
    let count = received.load(Ordering::Relaxed);
    assert!(
        count >= 6,
        "expected >= 6 messages (2 connects), got {count}"
    );
    assert!(
        handle.reconnects.load(Ordering::Relaxed) >= 1,
        "expected at least 1 reconnect"
    );
    handle.shutdown();
}

#[tokio::test]
async fn feed_thread_subscription_message() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let sub_received = Arc::new(AtomicU32::new(0));
    let sr = Arc::clone(&sub_received);

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let ws = accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg
                    && text.as_str().contains("subscribe")
                {
                    sr.fetch_add(1, Ordering::Relaxed);
                    let _ = write
                        .send(Message::Text(r#"{"status":"subscribed"}"#.into()))
                        .await;
                }
            }
        }
    });

    let config = FeedConfig {
        name: "sub-test".to_owned(),
        ws: WsConfig {
            url: format!("ws://{addr}"),
            subscribe_msg: Some(r#"{"type":"subscribe","channel":"test"}"#.to_owned()),
            ping_interval: None,
            read_timeout: Some(Duration::from_secs(2)),
        },
        tuning: SocketTuning::default(),
        backoff: BackoffConfig::default(),
    };

    let handle = FeedThread::spawn(config, |_msg| true).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(sub_received.load(Ordering::Relaxed), 1);
    handle.shutdown();
}
