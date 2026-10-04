//! The link that goes over the internet. The port's own.
//!
//! Each game has an `iroh` endpoint, which is dialled by its key and not by an
//! address: iroh finds a way between two of them, straight through both players'
//! routers where it can and by way of one of its relays where it can't, so nobody has
//! a port to forward. The relays used are iroh's public ones.
//!
//! A player's game opens one stream to the host for what must arrive (each message
//! with its length before it) and both send datagrams for what needn't. iroh wants
//! an async runtime and the game hasn't one, so all of this runs on `runtime`'s
//! threads and talks to the game through channels.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointId};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::link::{Event, Link};
use super::protocol::{HOST, Peer};

/// Games speak only to games that speak this.
const ALPN: &[u8] = b"brick-racers/1";
/// The byte a player's stream begins with.
const GREETING: u8 = 1;
/// The longest message taken from a stream.
const MAX_MESSAGE: usize = 64 * 1024;
/// How long a game is given to reach its host.
const DIAL_WAIT: Duration = Duration::from_secs(20);

/// The threads everything that waits on the network runs on.
pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        // The lobby is asked over HTTPS, and this is whose arithmetic that uses.
        let _ = rustls::crypto::ring::default_provider().install_default();
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("net").build().expect("threads for the network")
    })
}

enum Out {
    Message(Peer, Vec<u8>),
    Datagram(Peer, Vec<u8>),
}

pub struct Transport {
    out: UnboundedSender<Out>,
    events: Mutex<Receiver<Event>>,
    /// What other games dial to reach this one, once it is known.
    address: Arc<OnceLock<String>>,
}

impl Transport {
    /// A host's end: other games dial `address`.
    pub fn host() -> Self {
        Self::start(None)
    }

    /// A player's end, dialling the host at `address`.
    pub fn join(address: &str) -> Self {
        Self::start(Some(address.to_string()))
    }

    fn start(dial: Option<String>) -> Self {
        let (out, outgoing) = unbounded_channel();
        let (tell, events) = channel();
        let address = Arc::new(OnceLock::new());
        let known = address.clone();
        runtime().spawn(async move {
            let ran = match dial {
                Some(host) => join(&host, outgoing, tell.clone()).await,
                None => host(outgoing, tell.clone(), known).await,
            };
            if let Err(error) = ran {
                let _ = tell.send(Event::Failed(format!("{error:#}")));
            }
        });
        Transport { out, events: Mutex::new(events), address }
    }
}

impl Link for Transport {
    fn send(&mut self, to: Peer, bytes: Vec<u8>) {
        let _ = self.out.send(Out::Message(to, bytes));
    }

    fn datagram(&mut self, to: Peer, bytes: Vec<u8>) {
        let _ = self.out.send(Out::Datagram(to, bytes));
    }

    fn poll(&mut self) -> Option<Event> {
        self.events.lock().ok()?.try_recv().ok()
    }

    fn address(&self) -> Option<String> {
        self.address.get().cloned()
    }
}

async fn endpoint() -> Result<Endpoint> {
    Endpoint::builder(presets::N0).alpns(vec![ALPN.to_vec()]).bind().await.context("opening the network")
}

/// Writes each message handed to it down a stream, its length first.
async fn write(mut stream: SendStream, mut messages: UnboundedReceiver<Vec<u8>>) -> Result<()> {
    while let Some(message) = messages.recv().await {
        stream.write_all(&(message.len() as u32).to_le_bytes()).await?;
        stream.write_all(&message).await?;
    }
    Ok(())
}

/// Reads messages from a stream and datagrams from its connection until either ends.
async fn read(peer: Peer, connection: Connection, mut stream: RecvStream, tell: Sender<Event>) -> Result<()> {
    let messages = async {
        loop {
            let mut length = [0u8; 4];
            stream.read_exact(&mut length).await?;
            let length = u32::from_le_bytes(length) as usize;
            if length > MAX_MESSAGE {
                bail!("a message of {length} bytes");
            }
            let mut message = vec![0u8; length];
            stream.read_exact(&mut message).await?;
            if tell.send(Event::Message(peer, message)).is_err() {
                return Ok(());
            }
        }
    };
    let datagrams = async {
        loop {
            let datagram = connection.read_datagram().await?;
            if tell.send(Event::Datagram(peer, datagram.to_vec())).is_err() {
                return anyhow::Ok(());
            }
        }
    };
    tokio::select! {
        ended = messages => ended,
        ended = datagrams => ended,
    }
}

async fn join(host: &str, mut outgoing: UnboundedReceiver<Out>, tell: Sender<Event>) -> Result<()> {
    let host: EndpointId = host.parse().context("the host's address")?;
    let endpoint = endpoint().await?;
    let connection = tokio::time::timeout(DIAL_WAIT, endpoint.connect(host, ALPN)).await.context("the host didn't answer")?.context("reaching the host")?;
    let (mut send, receive) = connection.open_bi().await.context("opening a stream to the host")?;
    // A stream isn't there for the other end until something has been sent down it.
    send.write_all(&[GREETING]).await.context("greeting the host")?;
    let (messages, queued) = unbounded_channel();
    tokio::spawn(write(send, queued));
    tell.send(Event::Joined(HOST))?;
    let reading = read(HOST, connection.clone(), receive, tell.clone());
    let writing = async {
        while let Some(out) = outgoing.recv().await {
            match out {
                Out::Message(_, bytes) => drop(messages.send(bytes)),
                // One too big to send, or sent while the way is blocked, is one lost.
                Out::Datagram(_, bytes) => drop(connection.send_datagram(Bytes::from(bytes))),
            }
        }
    };
    tokio::select! {
        // The game has let go of its end.
        _ = writing => {}
        _ = reading => drop(tell.send(Event::Left(HOST))),
    }
    connection.close(0u32.into(), b"left");
    endpoint.close().await;
    Ok(())
}

async fn host(mut outgoing: UnboundedReceiver<Out>, tell: Sender<Event>, address: Arc<OnceLock<String>>) -> Result<()> {
    let endpoint = endpoint().await?;
    // Until it has a relay to be found through, nobody could dial it.
    endpoint.online().await;
    let _ = address.set(endpoint.id().to_string());
    type Players = Arc<Mutex<HashMap<Peer, (Connection, UnboundedSender<Vec<u8>>)>>>;
    let players: Players = Arc::default();
    let accepting = async {
        let mut joined: Peer = HOST;
        while let Some(incoming) = endpoint.accept().await {
            joined += 1;
            let (peer, players, tell) = (joined, players.clone(), tell.clone());
            tokio::spawn(async move {
                let met = async {
                    let connection = incoming.await.context("a player connecting")?;
                    let (send, mut receive) = connection.accept_bi().await.context("a player's stream")?;
                    let mut greeting = [0u8; 1];
                    receive.read_exact(&mut greeting).await.context("a player's greeting")?;
                    if greeting != [GREETING] {
                        bail!("a greeting of {greeting:?}");
                    }
                    let (messages, queued) = unbounded_channel();
                    tokio::spawn(write(send, queued));
                    players.lock().unwrap().insert(peer, (connection.clone(), messages));
                    tell.send(Event::Joined(peer))?;
                    read(peer, connection, receive, tell.clone()).await
                };
                if let Err(error) = met.await {
                    bevy::log::debug!("player {peer}: {error:#}");
                }
                if players.lock().unwrap().remove(&peer).is_some() {
                    let _ = tell.send(Event::Left(peer));
                }
            });
        }
    };
    let writing = async {
        while let Some(out) = outgoing.recv().await {
            let players = players.lock().unwrap();
            match out {
                Out::Message(peer, bytes) => drop(players.get(&peer).map(|(_, messages)| messages.send(bytes))),
                Out::Datagram(peer, bytes) => drop(players.get(&peer).map(|(connection, _)| connection.send_datagram(Bytes::from(bytes)))),
            }
        }
    };
    tokio::select! {
        _ = writing => {}
        _ = accepting => {}
    }
    endpoint.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait_for(link: &mut Transport, what: &str, wanted: impl Fn(&Event) -> bool) -> Event {
        let began = Instant::now();
        loop {
            match link.poll() {
                Some(Event::Failed(why)) => panic!("{what}: {why}"),
                Some(event) if wanted(&event) => return event,
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
            assert!(began.elapsed() < Duration::from_secs(30), "waited too long for {what}");
        }
    }

    /// Two real endpoints on this machine, found through iroh's relays: it needs the
    /// internet, so it is run by hand (`cargo test real_endpoints -- --ignored`).
    #[test]
    #[ignore = "needs the internet"]
    fn real_endpoints_find_each_other_and_talk() {
        let mut host = Transport::host();
        let began = Instant::now();
        while host.address().is_none() {
            if let Some(Event::Failed(why)) = host.poll() {
                panic!("hosting: {why}");
            }
            assert!(began.elapsed() < Duration::from_secs(30), "the host never came online");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut player = Transport::join(&host.address().unwrap());
        wait_for(&mut player, "the player to reach the host", |e| *e == Event::Joined(HOST));
        let Event::Joined(peer) = wait_for(&mut host, "the host to hear of the player", |e| matches!(e, Event::Joined(_))) else { unreachable!() };

        player.send(HOST, vec![1, 2, 3]);
        player.send(HOST, vec![4; 5000]);
        assert_eq!(wait_for(&mut host, "the first message", |e| matches!(e, Event::Message(..))), Event::Message(peer, vec![1, 2, 3]));
        assert_eq!(wait_for(&mut host, "the second message", |e| matches!(e, Event::Message(..))), Event::Message(peer, vec![4; 5000]));
        host.send(peer, vec![9]);
        assert_eq!(wait_for(&mut player, "the host's answer", |e| matches!(e, Event::Message(..))), Event::Message(HOST, vec![9]));

        // Datagrams may be lost, so a few are sent each way; a snapshot's worth fits.
        for _ in 0..20 {
            player.datagram(HOST, vec![7; 60]);
            host.datagram(peer, vec![8; 1000]);
        }
        assert_eq!(wait_for(&mut host, "a datagram", |e| matches!(e, Event::Datagram(..))), Event::Datagram(peer, vec![7; 60]));
        assert_eq!(wait_for(&mut player, "a datagram back", |e| matches!(e, Event::Datagram(..))), Event::Datagram(HOST, vec![8; 1000]));

        // A player who goes is heard to have gone.
        drop(player);
        assert_eq!(wait_for(&mut host, "the player leaving", |e| matches!(e, Event::Left(_))), Event::Left(peer));
        println!("hosted, joined and talked in {:?}", began.elapsed());
    }
}
