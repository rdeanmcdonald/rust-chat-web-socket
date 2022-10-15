use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use config::Config;
use futures::future::join_all;
use futures::{
    sink::SinkExt,
    stream::{SplitSink, StreamExt},
};
use redis::AsyncCommands;
use serde::de::IntoDeserializer;
use std::{
    collections::HashSet,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod app_config;

const SESH_ID: &str = "14412d14-c21d-4f00-a74e-f02146e16f48";
// Our shared state
struct AppState {
    sesh_set: Mutex<HashSet<String>>,
    redis_client: redis::Client,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            // example log str to get trace logs for kafka client:
            // RUST_LOG="librdkafka=trace,rdkafka::client=debug"
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = app_config::get_config();
    let redis_url = format!("redis://{}/", config.get_string("redis.host").unwrap());
    let redis_client = redis::Client::open(redis_url).unwrap();

    let sesh_set = Mutex::new(HashSet::new());

    let app_state = Arc::new(AppState {
        sesh_set,
        redis_client,
    });

    let app = Router::with_state(app_state)
        .route("/", get(index))
        .route("/ws", get(websocket_handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sesh_id = SESH_ID;
    {
        // new scope to release lock after scope
        let mut sesh_set = state.sesh_set.lock().unwrap();
        if !sesh_set.contains(sesh_id) {
            sesh_set.insert(sesh_id.to_owned());
        }
    }
    ws.on_upgrade(|socket| websocket(socket, state, sesh_id.to_string()))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>, sesh_id: String) {
    let (sender, _receiver) = stream.split();

    let sesh_id = SESH_ID.to_string();
    let redis_conn = state.redis_client.get_async_connection().await.unwrap();

    // NOTE: Somehow we need to close the subber when the web socket closes
    let subber_handle = tokio::spawn(subber(redis_conn, sesh_id.clone(), sender));

    let handles = vec![subber_handle];
    let _results = join_all(handles).await;

    // Remove session from map once subber is done
    state.sesh_set.lock().unwrap().remove(&sesh_id);
}

// Include utf-8 file at **compile** time.
async fn index() -> Html<&'static str> {
    Html(std::include_str!("../chat.html"))
}

async fn subber(
    redis_conn: redis::aio::Connection,
    sesh_id: String,
    mut sender: SplitSink<WebSocket, Message>,
) {
    let mut pubsub = redis_conn.into_pubsub();
    let uuid = uuid::Uuid::parse_str(&sesh_id).unwrap();
    let _result = pubsub.subscribe(uuid.as_bytes()).await.unwrap();
    while let Some(msg) = pubsub.on_message().next().await {
        tracing::debug!("GOT MESSAGE FROM REDIS: {:?}", msg);
        let res = std::str::from_utf8(&msg.get_payload_bytes()).unwrap();
        if sender.send(Message::Text(res.to_string())).await.is_err() {
            panic!("ERROR SENDING PUBSUB MSG TO WEB SOCKET")
        }
    }
}
