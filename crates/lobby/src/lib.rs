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
//! | `GET /` | a page saying what this is, where its source is, and how busy it is |
//! | `GET /favicon.ico` | the picture a browser puts beside that page: a brick |
//!
//! Nothing is kept across a restart: a host whose session has gone is told so by its
//! next beat (404) and lists it again.

pub mod tracing;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Html;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use lobby_api::{
    GONE_AFTER, MAX_ENDPOINT, MAX_NAME, MAX_PLAYERS, Register, Registered, Session, Status,
};
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

/// The picture a browser shows beside the page: a brick from above, drawn for this.
const FAVICON: &[u8] = include_bytes!("favicon.png");

/// Where the game and this server come from.
const SOURCE: &str = "https://github.com/jchevertonwynne/blockracers";

pub struct Lobby {
    sessions: Mutex<HashMap<String, Entry>>,
    listed: AtomicU64,
    refused: AtomicU64,
    started: Instant,
}

impl Default for Lobby {
    fn default() -> Self {
        Lobby {
            sessions: Mutex::default(),
            listed: AtomicU64::default(),
            refused: AtomicU64::default(),
            started: Instant::now(),
        }
    }
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
        .route("/", get(home))
        .route(
            "/favicon.ico",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "image/png"),
                        (header::CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    FAVICON,
                )
            }),
        )
        .route("/healthz", get(|| async { "ok" }))
        .route("/metrics", get(metrics))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(axum::middleware::from_fn(tracing::trace))
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
    status.players <= max
        && status.circuit.len() <= MAX_NAME
        && !status.circuit.chars().any(char::is_control)
}

async fn register(
    State(lobby): State<Shared>,
    headers: HeaderMap,
    Json(ask): Json<Register>,
) -> Result<Json<Registered>, StatusCode> {
    let fits = named(&ask.name)
        && named(&ask.host)
        && !ask.endpoint.is_empty()
        && ask.endpoint.len() <= MAX_ENDPOINT
        && (1..=MAX_PLAYERS).contains(&ask.max)
        && sound(&ask.status, ask.max);
    if !fits {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let address = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut sessions = lobby.live();
    let theirs = sessions
        .values()
        .filter(|e| !address.is_empty() && e.address == address)
        .count();
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
    sessions.insert(
        id.clone(),
        Entry {
            session,
            protocol: ask.protocol,
            token: token.clone(),
            address,
            heard: Instant::now(),
        },
    );
    lobby.listed.fetch_add(1, Ordering::Relaxed);
    Ok(Json(Registered { id, token }))
}

/// The session `id`, if the request carries its token. A session that isn't there and
/// a token that is wrong are told apart, so a host knows when to list itself again.
fn owned<'a>(
    sessions: &'a mut HashMap<String, Entry>,
    id: &str,
    headers: &HeaderMap,
) -> Result<&'a mut Entry, StatusCode> {
    let entry = sessions.get_mut(id).ok_or(StatusCode::NOT_FOUND)?;
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if given != Some(entry.token.as_str()) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(entry)
}

async fn beat(
    State(lobby): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(status): Json<Status>,
) -> Result<StatusCode, StatusCode> {
    let mut sessions = lobby.live();
    let entry = owned(&mut sessions, &id, &headers)?;
    if !sound(&status, entry.session.max) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    (entry.session.status, entry.heard) = (status, Instant::now());
    Ok(StatusCode::NO_CONTENT)
}

async fn close(
    State(lobby): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
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
    let mut found: Vec<Session> = sessions
        .values()
        .filter(|e| e.protocol == which.protocol)
        .map(|e| e.session.clone())
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Json(found)
}

/// Text made safe to put in a page: a session's name is whatever its host typed.
fn escaped(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, c| {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
        out
    })
}

/// So many seconds, as a person would say it.
fn span(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        3600..86400 => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{} d {} h", seconds / 86400, seconds % 86400 / 3600),
    }
}

/// The page at the root: what this is, where its source is, and how busy it is. How
/// a session is dialled is left off it; the game reads that from `/sessions`.
async fn home(State(lobby): State<Shared>) -> Html<String> {
    let mut sessions: Vec<Session> = lobby.live().values().map(|e| e.session.clone()).collect();
    sessions.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    let players: usize = sessions
        .iter()
        .map(|session| session.status.players as usize)
        .sum();
    let racing = sessions
        .iter()
        .filter(|session| session.status.racing)
        .count();
    let rows: String = sessions
        .iter()
        .map(|session| {
            let doing = match (session.status.racing, session.status.circuit.is_empty()) {
                (true, false) => format!("racing {}", escaped(&session.status.circuit)),
                (true, true) => "racing".to_string(),
                (false, _) => "in the room".to_string(),
            };
            let locked = if session.locked { "password" } else { "open" };
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}/{}</td><td>{doing}</td><td>{locked}</td></tr>",
                escaped(&session.name),
                escaped(&session.host),
                session.status.players,
                session.max
            )
        })
        .collect();
    let table = if sessions.is_empty() {
        "<p>Nobody is hosting a race just now.</p>".to_string()
    } else {
        format!(
            "<table><tr><th>Session</th><th>Host</th><th>Players</th><th>Now</th><th>Entry</th></tr>{rows}</table>"
        )
    };
    Html(format!(
        r#"<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>Brick Racers lobby</title>
<link rel="icon" type="image/png" href="/favicon.ico">
<style>
body {{ font: 16px/1.5 system-ui, sans-serif; max-width: 44rem; margin: 3rem auto; padding: 0 1rem; }}
dl {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr)); gap: 1rem; }}
dt {{ font-size: .8rem; opacity: .7; }}
dd {{ margin: 0; font-size: 1.6rem; font-variant-numeric: tabular-nums; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ text-align: left; padding: .35rem .6rem .35rem 0; border-bottom: 1px solid color-mix(in srgb, currentColor 20%, transparent); }}
</style>
<h1>Brick Racers lobby</h1>
<p>The list of races being hosted online in Brick Racers. This server only keeps the list: players connect to each other directly, and nothing of a race passes through here.</p>
<p>The game and this server: <a href="{SOURCE}">{SOURCE}</a></p>
<h2>Now</h2>
<dl>
<div><dt>Sessions</dt><dd>{now}</dd></div>
<div><dt>Players</dt><dd>{players}</dd></div>
<div><dt>Racing</dt><dd>{racing}</dd></div>
</dl>
{table}
<h2>Since this server started</h2>
<dl>
<div><dt>Up for</dt><dd>{up}</dd></div>
<div><dt>Sessions hosted</dt><dd>{listed}</dd></div>
<div><dt>Turned away</dt><dd>{refused}</dd></div>
</dl>
</html>
"#,
        now = sessions.len(),
        up = span(lobby.started.elapsed().as_secs()),
        listed = lobby.listed.load(Ordering::Relaxed),
        refused = lobby.refused.load(Ordering::Relaxed),
    ))
}

async fn metrics(State(lobby): State<Shared>) -> String {
    let (sessions, players) = {
        let sessions = lobby.live();
        (
            sessions.len(),
            sessions
                .values()
                .map(|e| e.session.status.players as usize)
                .sum::<usize>(),
        )
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
            status: Status {
                players: 1,
                ..Status::default()
            },
        }
    }

    /// Sends one request and gives back the answer's status and body.
    async fn send(
        app: &Router,
        method: &str,
        path: &str,
        token: Option<&str>,
        address: Option<&str>,
        body: Option<String>,
    ) -> (StatusCode, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(address) = address {
            request = request.header("cf-connecting-ip", address);
        }
        let answer = app
            .clone()
            .oneshot(request.body(Body::from(body.unwrap_or_default())).unwrap())
            .await
            .unwrap();
        let status = answer.status();
        let bytes = answer.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn listing(app: &Router, protocol: u32) -> Vec<Session> {
        let (status, body) = send(
            app,
            "GET",
            &format!("/sessions?protocol={protocol}"),
            None,
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_str(&body).unwrap()
    }

    async fn host(app: &Router, ask: &Register, address: Option<&str>) -> Registered {
        let (status, body) = send(
            app,
            "POST",
            "/sessions",
            None,
            address,
            Some(serde_json::to_string(ask).unwrap()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_str(&body).unwrap()
    }

    #[tokio::test]
    async fn a_listed_session_is_seen_by_games_of_its_protocol() {
        let app = app();
        let made = host(
            &app,
            &Register {
                locked: true,
                ..ask("Friday night")
            },
            None,
        )
        .await;
        let seen = listing(&app, PROTOCOL).await;
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].id.as_str(), seen[0].name.as_str(), seen[0].locked),
            (made.id.as_str(), "Friday night", true)
        );
        assert_eq!(seen[0].endpoint, "somewhere");
        assert!(listing(&app, PROTOCOL + 1).await.is_empty());
        // The host's token is not on the list.
        let (_, raw) = send(
            &app,
            "GET",
            &format!("/sessions?protocol={PROTOCOL}"),
            None,
            None,
            None,
        )
        .await;
        assert!(!raw.contains(&made.token));
    }

    #[tokio::test(start_paused = true)]
    async fn a_session_stays_while_its_host_is_heard_from() {
        let app = app();
        let made = host(&app, &ask("Quiet"), None).await;
        let path = format!("/sessions/{}", made.id);
        let status = serde_json::to_string(&Status {
            players: 3,
            circuit: "RACEC0R0".into(),
            racing: true,
        })
        .unwrap();

        tokio::time::advance(Duration::from_secs(GONE_AFTER - 1)).await;
        assert_eq!(
            send(
                &app,
                "PUT",
                &path,
                Some(&made.token),
                None,
                Some(status.clone())
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        tokio::time::advance(Duration::from_secs(GONE_AFTER - 1)).await;
        let seen = listing(&app, PROTOCOL).await;
        assert_eq!((seen[0].status.players, seen[0].status.racing), (3, true));

        // Unheard from, it goes, and its host's next beat is told so.
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(listing(&app, PROTOCOL).await.is_empty());
        assert_eq!(
            send(&app, "PUT", &path, Some(&made.token), None, Some(status))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_root_says_what_this_is_and_how_busy() {
        let app = app();
        let (status, empty) = send(&app, "GET", "/", None, None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            empty.contains(&format!("<a href=\"{SOURCE}\">"))
                && empty.contains("Nobody is hosting")
        );

        // A name is whatever its host typed, and is shown as typed, not acted on.
        let racing = Status {
            players: 3,
            circuit: "Royal Knights Raceway".into(),
            racing: true,
        };
        host(
            &app,
            &Register {
                name: "<b>Friday</b> & co".into(),
                locked: true,
                status: racing,
                ..ask("x")
            },
            None,
        )
        .await;
        host(&app, &ask("Quiet"), None).await;
        tokio::time::advance(Duration::from_secs(3700)).await;
        let made = host(&app, &ask("Late"), None).await;
        let (_, page) = send(&app, "GET", "/", None, None, None).await;
        // The two from an hour ago have gone unheard from; the page shows what is there now.
        assert!(!page.contains("Friday") && page.contains("<td>Late</td>"));
        assert!(
            page.contains("<dt>Sessions</dt><dd>1</dd>")
                && page.contains("<dt>Sessions hosted</dt><dd>3</dd>")
        );
        assert!(page.contains("<dd>1 h 1 min</dd>"));
        // How to dial a session and its host's token are not on the page.
        assert!(!page.contains("somewhere") && !page.contains(&made.token));

        let busy = Status {
            players: 3,
            circuit: "Royal Knights Raceway".into(),
            racing: true,
        };
        host(
            &app,
            &Register {
                name: "<b>Friday</b> & co".into(),
                locked: true,
                status: busy,
                ..ask("x")
            },
            None,
        )
        .await;
        let (_, page) = send(&app, "GET", "/", None, None, None).await;
        assert!(
            page.contains("<td>&lt;b&gt;Friday&lt;/b&gt; &amp; co</td>")
                && !page.contains("<b>Friday")
        );
        assert!(
            page.contains("<td>3/6</td><td>racing Royal Knights Raceway</td><td>password</td>")
        );
        assert!(
            page.contains("<dt>Players</dt><dd>4</dd>")
                && page.contains("<dt>Racing</dt><dd>1</dd>")
        );
    }

    #[tokio::test]
    async fn a_browser_is_given_a_picture_for_the_page() {
        let app = app();
        let request = Request::builder()
            .uri("/favicon.ico")
            .body(Body::empty())
            .unwrap();
        let answer = app.clone().oneshot(request).await.unwrap();
        assert_eq!(answer.status(), StatusCode::OK);
        assert_eq!(answer.headers()[header::CONTENT_TYPE], "image/png");
        let bytes = answer.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() == FAVICON.len());
        // And the page says where it is.
        let (_, page) = send(&app, "GET", "/", None, None, None).await;
        assert!(page.contains(r#"<link rel="icon" type="image/png" href="/favicon.ico">"#));
    }

    #[test]
    fn a_span_of_time_is_said_as_a_person_would() {
        assert_eq!(
            [span(5), span(125), span(3725), span(90_000)],
            ["5 s", "2 min", "1 h 2 min", "1 d 1 h"]
        );
    }

    #[tokio::test]
    async fn only_its_host_changes_a_session_or_takes_it_down() {
        let app = app();
        let made = host(&app, &ask("Mine"), None).await;
        let path = format!("/sessions/{}", made.id);
        let status = serde_json::to_string(&Status::default()).unwrap();
        assert_eq!(
            send(
                &app,
                "PUT",
                &path,
                Some("guess"),
                None,
                Some(status.clone())
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(&app, "DELETE", &path, None, None, None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(listing(&app, PROTOCOL).await.len(), 1);
        // More players than the session has room for is not a status.
        let crowd = serde_json::to_string(&Status {
            players: 7,
            ..Status::default()
        })
        .unwrap();
        assert_eq!(
            send(&app, "PUT", &path, Some(&made.token), None, Some(crowd))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            send(&app, "DELETE", &path, Some(&made.token), None, None)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert!(listing(&app, PROTOCOL).await.is_empty());
    }

    #[tokio::test]
    async fn the_list_cannot_be_filled() {
        let app = app();
        let try_host = async |ask: Register, address: Option<&str>| {
            send(
                &app,
                "POST",
                "/sessions",
                None,
                address,
                Some(serde_json::to_string(&ask).unwrap()),
            )
            .await
            .0
        };
        // What doesn't fit is turned away.
        assert_eq!(
            try_host(ask(" "), None).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            try_host(ask(&"n".repeat(MAX_NAME + 1)), None).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            try_host(
                Register {
                    max: 7,
                    ..ask("Big")
                },
                None
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            try_host(
                Register {
                    endpoint: "e".repeat(MAX_ENDPOINT + 1),
                    ..ask("Long")
                },
                None
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let huge = format!("{{\"name\":\"{}\"}}", "x".repeat(MAX_BODY));
        assert_eq!(
            send(&app, "POST", "/sessions", None, None, Some(huge))
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );

        // One address lists only so many.
        for n in 0..MAX_PER_ADDRESS {
            assert_eq!(
                try_host(ask(&format!("Home {n}")), Some("203.0.113.7")).await,
                StatusCode::OK
            );
        }
        assert_eq!(
            try_host(ask("One more"), Some("203.0.113.7")).await,
            StatusCode::TOO_MANY_REQUESTS
        );

        // And the list holds only so many from everyone.
        for n in MAX_PER_ADDRESS..MAX_SESSIONS {
            assert_eq!(
                try_host(ask(&format!("Away {n}")), Some(&format!("198.51.100.{n}"))).await,
                StatusCode::OK
            );
        }
        assert_eq!(
            try_host(ask("Full"), Some("192.0.2.1")).await,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(listing(&app, PROTOCOL).await.len(), MAX_SESSIONS);

        let (_, metrics) = send(&app, "GET", "/metrics", None, None, None).await;
        assert!(metrics.contains(&format!("lobby_sessions {MAX_SESSIONS}\n")));
        assert!(metrics.contains("lobby_sessions_refused_total 2\n"));
        assert_eq!(
            send(&app, "GET", "/healthz", None, None, None).await,
            (StatusCode::OK, "ok".to_string())
        );
    }
}
