//! The lobby: the list of sessions being hosted, kept in memory. See `lobby_api` for
//! what is said to it. It carries none of a race's traffic; players dial each other.
//!
//! | Route | Does |
//! |---|---|
//! | `POST /sessions` | lists a session, answering with its id and the host's token |
//! | `PUT /sessions/{id}` | the host saying it is still there, with its `Status` |
//! | `DELETE /sessions/{id}` | the host taking it down |
//! | `GET /sessions?protocol=N` | the list |
//! | `GET /healthz`, `GET /metrics` | for the cluster's probes and scraping |
//!
//! Nothing is kept across a restart: a host whose session has gone is told so by its
//! next beat (404) and lists it again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use lobby_api::{GONE_AFTER, MAX_ENDPOINT, MAX_NAME, MAX_PLAYERS, Register, Registered, Session, Status};
use serde::Deserialize;
use tokio::time::Instant;

/// The most sessions listed at once, and the most from one address. The lobby is open
/// to anyone, so both are there to keep the list from being filled.
pub const MAX_SESSIONS: usize = 64;
pub const MAX_PER_ADDRESS: usize = 4;
/// The largest request body taken, in bytes.
const MAX_BODY: usize = 4096;

struct Entry {
    session: Session,
    protocol: u32,
    token: String,
    /// Who listed it, as Cloudflare names them; empty when it doesn't say.
    address: String,
    heard: Instant,
}

#[derive(Default)]
pub struct Lobby {
    sessions: Mutex<HashMap<String, Entry>>,
    listed: AtomicU64,
    refused: AtomicU64,
}

type Shared = Arc<Lobby>;

impl Lobby {
    /// The sessions, less those not heard from in `GONE_AFTER`.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        let mut sessions = self.sessions.lock().unwrap();
        let now = Instant::now();
        sessions.retain(|_, e| now.duration_since(e.heard) < Duration::from_secs(GONE_AFTER));
        sessions
    }
}

pub fn app() -> Router {
    Router::new()
        .route("/sessions", post(register).get(list))
        .route("/sessions/{id}", put(beat).delete(close))
        .route("/healthz", get(|| async { "ok" }))
        .route("/metrics", get(metrics))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(Shared::default())
}

/// Sixteen random bytes, in hex.
fn random_name() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A name is short, not empty, and has nothing in it a list couldn't show.
fn named(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
}

fn sound(status: &Status, max: u8) -> bool {
    status.players <= max && status.circuit.len() <= MAX_NAME && !status.circuit.chars().any(char::is_control)
}

async fn register(State(lobby): State<Shared>, headers: HeaderMap, Json(ask): Json<Register>) -> Result<Json<Registered>, StatusCode> {
    let fits = named(&ask.name)
        && named(&ask.host)
        && !ask.endpoint.is_empty()
        && ask.endpoint.len() <= MAX_ENDPOINT
        && (1..=MAX_PLAYERS).contains(&ask.max)
        && sound(&ask.status, ask.max);
    if !fits {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let address = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let mut sessions = lobby.live();
    let theirs = sessions.values().filter(|e| !address.is_empty() && e.address == address).count();
    if sessions.len() >= MAX_SESSIONS || theirs >= MAX_PER_ADDRESS {
        lobby.refused.fetch_add(1, Ordering::Relaxed);
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let (id, token) = (random_name(), random_name());
    let session = Session {
        id: id.clone(),
        name: ask.name,
        host: ask.host,
        endpoint: ask.endpoint,
        locked: ask.locked,
        max: ask.max,
        status: ask.status,
    };
    sessions.insert(id.clone(), Entry { session, protocol: ask.protocol, token: token.clone(), address, heard: Instant::now() });
    lobby.listed.fetch_add(1, Ordering::Relaxed);
    Ok(Json(Registered { id, token }))
}

/// The session `id`, if the request carries its token. A session that isn't there and
/// a token that is wrong are told apart, so a host knows when to list itself again.
fn owned<'a>(sessions: &'a mut HashMap<String, Entry>, id: &str, headers: &HeaderMap) -> Result<&'a mut Entry, StatusCode> {
    let entry = sessions.get_mut(id).ok_or(StatusCode::NOT_FOUND)?;
    let given = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if given != Some(entry.token.as_str()) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(entry)
}

async fn beat(State(lobby): State<Shared>, Path(id): Path<String>, headers: HeaderMap, Json(status): Json<Status>) -> Result<StatusCode, StatusCode> {
    let mut sessions = lobby.live();
    let entry = owned(&mut sessions, &id, &headers)?;
    if !sound(&status, entry.session.max) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    (entry.session.status, entry.heard) = (status, Instant::now());
    Ok(StatusCode::NO_CONTENT)
}

async fn close(State(lobby): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<StatusCode, StatusCode> {
    let mut sessions = lobby.live();
    owned(&mut sessions, &id, &headers)?;
    sessions.remove(&id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Which {
    protocol: u32,
}

async fn list(State(lobby): State<Shared>, Query(which): Query<Which>) -> Json<Vec<Session>> {
    let sessions = lobby.live();
    let mut found: Vec<Session> = sessions.values().filter(|e| e.protocol == which.protocol).map(|e| e.session.clone()).collect();
    found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Json(found)
}

async fn metrics(State(lobby): State<Shared>) -> String {
    let (sessions, players) = {
        let sessions = lobby.live();
        (sessions.len(), sessions.values().map(|e| e.session.status.players as usize).sum::<usize>())
    };
    format!(
        "# TYPE lobby_sessions gauge\nlobby_sessions {sessions}\n\
         # TYPE lobby_players gauge\nlobby_players {players}\n\
         # TYPE lobby_sessions_listed_total counter\nlobby_sessions_listed_total {}\n\
         # TYPE lobby_sessions_refused_total counter\nlobby_sessions_refused_total {}\n",
        lobby.listed.load(Ordering::Relaxed),
        lobby.refused.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use lobby_api::PROTOCOL;
    use tower::ServiceExt;

    fn ask(name: &str) -> Register {
        Register {
            protocol: PROTOCOL,
            name: name.into(),
            host: "Rocket Racer".into(),
            endpoint: "somewhere".into(),
            locked: false,
            max: 6,
            status: Status { players: 1, ..Status::default() },
        }
    }

    /// Sends one request and gives back the answer's status and body.
    async fn send(app: &Router, method: &str, path: &str, token: Option<&str>, address: Option<&str>, body: Option<String>) -> (StatusCode, String) {
        let mut request = Request::builder().method(method).uri(path).header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(address) = address {
            request = request.header("cf-connecting-ip", address);
        }
        let answer = app.clone().oneshot(request.body(Body::from(body.unwrap_or_default())).unwrap()).await.unwrap();
        let status = answer.status();
        let bytes = answer.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn listing(app: &Router, protocol: u32) -> Vec<Session> {
        let (status, body) = send(app, "GET", &format!("/sessions?protocol={protocol}"), None, None, None).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_str(&body).unwrap()
    }

    async fn host(app: &Router, ask: &Register, address: Option<&str>) -> Registered {
        let (status, body) = send(app, "POST", "/sessions", None, address, Some(serde_json::to_string(ask).unwrap())).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_str(&body).unwrap()
    }

    #[tokio::test]
    async fn a_listed_session_is_seen_by_games_of_its_protocol() {
        let app = app();
        let made = host(&app, &Register { locked: true, ..ask("Friday night") }, None).await;
        let seen = listing(&app, PROTOCOL).await;
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].id.as_str(), seen[0].name.as_str(), seen[0].locked), (made.id.as_str(), "Friday night", true));
        assert_eq!(seen[0].endpoint, "somewhere");
        assert!(listing(&app, PROTOCOL + 1).await.is_empty());
        // The host's token is not on the list.
        let (_, raw) = send(&app, "GET", &format!("/sessions?protocol={PROTOCOL}"), None, None, None).await;
        assert!(!raw.contains(&made.token));
    }

    #[tokio::test(start_paused = true)]
    async fn a_session_stays_while_its_host_is_heard_from() {
        let app = app();
        let made = host(&app, &ask("Quiet"), None).await;
        let path = format!("/sessions/{}", made.id);
        let status = serde_json::to_string(&Status { players: 3, circuit: "RACEC0R0".into(), racing: true }).unwrap();

        tokio::time::advance(Duration::from_secs(GONE_AFTER - 1)).await;
        assert_eq!(send(&app, "PUT", &path, Some(&made.token), None, Some(status.clone())).await.0, StatusCode::NO_CONTENT);
        tokio::time::advance(Duration::from_secs(GONE_AFTER - 1)).await;
        let seen = listing(&app, PROTOCOL).await;
        assert_eq!((seen[0].status.players, seen[0].status.racing), (3, true));

        // Unheard from, it goes, and its host's next beat is told so.
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(listing(&app, PROTOCOL).await.is_empty());
        assert_eq!(send(&app, "PUT", &path, Some(&made.token), None, Some(status)).await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_its_host_changes_a_session_or_takes_it_down() {
        let app = app();
        let made = host(&app, &ask("Mine"), None).await;
        let path = format!("/sessions/{}", made.id);
        let status = serde_json::to_string(&Status::default()).unwrap();
        assert_eq!(send(&app, "PUT", &path, Some("guess"), None, Some(status.clone())).await.0, StatusCode::FORBIDDEN);
        assert_eq!(send(&app, "DELETE", &path, None, None, None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(listing(&app, PROTOCOL).await.len(), 1);
        // More players than the session has room for is not a status.
        let crowd = serde_json::to_string(&Status { players: 7, ..Status::default() }).unwrap();
        assert_eq!(send(&app, "PUT", &path, Some(&made.token), None, Some(crowd)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(send(&app, "DELETE", &path, Some(&made.token), None, None).await.0, StatusCode::NO_CONTENT);
        assert!(listing(&app, PROTOCOL).await.is_empty());
    }

    #[tokio::test]
    async fn the_list_cannot_be_filled() {
        let app = app();
        let try_host = async |ask: Register, address: Option<&str>| send(&app, "POST", "/sessions", None, address, Some(serde_json::to_string(&ask).unwrap())).await.0;
        // What doesn't fit is turned away.
        assert_eq!(try_host(ask(" "), None).await, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(try_host(ask(&"n".repeat(MAX_NAME + 1)), None).await, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(try_host(Register { max: 7, ..ask("Big") }, None).await, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(try_host(Register { endpoint: "e".repeat(MAX_ENDPOINT + 1), ..ask("Long") }, None).await, StatusCode::UNPROCESSABLE_ENTITY);
        let huge = format!("{{\"name\":\"{}\"}}", "x".repeat(MAX_BODY));
        assert_eq!(send(&app, "POST", "/sessions", None, None, Some(huge)).await.0, StatusCode::PAYLOAD_TOO_LARGE);

        // One address lists only so many.
        for n in 0..MAX_PER_ADDRESS {
            assert_eq!(try_host(ask(&format!("Home {n}")), Some("203.0.113.7")).await, StatusCode::OK);
        }
        assert_eq!(try_host(ask("One more"), Some("203.0.113.7")).await, StatusCode::TOO_MANY_REQUESTS);

        // And the list holds only so many from everyone.
        for n in MAX_PER_ADDRESS..MAX_SESSIONS {
            assert_eq!(try_host(ask(&format!("Away {n}")), Some(&format!("198.51.100.{n}"))).await, StatusCode::OK);
        }
        assert_eq!(try_host(ask("Full"), Some("192.0.2.1")).await, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(listing(&app, PROTOCOL).await.len(), MAX_SESSIONS);

        let (_, metrics) = send(&app, "GET", "/metrics", None, None, None).await;
        assert!(metrics.contains(&format!("lobby_sessions {MAX_SESSIONS}\n")));
        assert!(metrics.contains("lobby_sessions_refused_total 2\n"));
        assert_eq!(send(&app, "GET", "/healthz", None, None, None).await, (StatusCode::OK, "ok".to_string()));
    }
}
