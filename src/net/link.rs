//! What carries messages between a host and its players. The port's own.
//!
//! The game speaks to a `Link` and doesn't know what is behind it: the real thing
//! (`transport`), or for tests one in memory that can be made as slow and as lossy as
//! a bad connection.

use serde::{Deserialize, Serialize};

use super::protocol::Peer;

/// How good the way to another game is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// There and back, in milliseconds.
    pub ping: u16,
    /// Straight between the two games, and not by way of a relay.
    pub direct: bool,
}

/// Something a link has to tell.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A player has connected to the host, or a player's game has reached its host.
    Joined(Peer),
    Left(Peer),
    /// Bytes sent with `send`: all of them arrive, in the order they were sent.
    Message(Peer, Vec<u8>),
    /// Bytes sent with `datagram`: some may not arrive, and not in order.
    Datagram(Peer, Vec<u8>),
    /// The link itself couldn't be made, or has broken: why.
    Failed(String),
}

pub trait Link: Send + Sync + 'static {
    fn send(&mut self, to: Peer, bytes: Vec<u8>);
    fn datagram(&mut self, to: Peer, bytes: Vec<u8>);
    /// The next thing to have happened, if anything has.
    fn poll(&mut self) -> Option<Event>;
    /// What another game dials to reach this end, if it can be dialled and once that
    /// is known.
    fn address(&self) -> Option<String> {
        None
    }
    /// How good the way to `peer` is, once that is known.
    fn quality(&self, _peer: Peer) -> Option<Quality> {
        None
    }
    /// Lets go of a player, once what has been sent them has gone. Nothing says
    /// they have `Left`: whoever closes it knows.
    fn close(&mut self, _peer: Peer) {}
}

#[cfg(test)]
pub use memory::Hub;

#[cfg(test)]
mod memory {
    use super::*;
    use crate::meshgen::Rng;
    use crate::net::protocol::HOST;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Inner {
        now: u32,
        sent: u64,
        /// What is on its way to each end: when it arrives, the order it was sent in,
        /// and what it is.
        queues: HashMap<Peer, Vec<(u32, u64, Event)>>,
        joined: Peer,
    }

    /// A network in memory. Everything takes `delay` steps to arrive, and `loss` of
    /// the datagrams don't.
    #[derive(Clone)]
    pub struct Hub {
        inner: Arc<Mutex<Inner>>,
        pub delay: u32,
        pub loss: f32,
    }

    pub struct Memory {
        hub: Hub,
        me: Peer,
        dice: Rng,
    }

    impl Hub {
        pub fn new(delay: u32, loss: f32) -> Self {
            Hub {
                inner: Arc::default(),
                delay,
                loss,
            }
        }

        /// Time passes.
        pub fn step(&self) {
            self.inner.lock().unwrap().now += 1;
        }

        pub fn host(&self) -> Memory {
            Memory {
                hub: self.clone(),
                me: HOST,
                dice: Rng(0x5eed),
            }
        }

        /// A player's end, which the host hears has joined.
        pub fn join(&self) -> Memory {
            let me = {
                let mut inner = self.inner.lock().unwrap();
                inner.joined += 1;
                inner.joined
            };
            let link = Memory {
                hub: self.clone(),
                me,
                dice: Rng(0xfeed + me),
            };
            link.post(HOST, Event::Joined(me));
            link.post(me, Event::Joined(HOST));
            link
        }
    }

    impl Memory {
        pub fn peer(&self) -> Peer {
            self.me
        }

        fn post(&self, to: Peer, event: Event) {
            let mut inner = self.hub.inner.lock().unwrap();
            inner.sent += 1;
            let (due, order) = (inner.now + self.hub.delay, inner.sent);
            inner
                .queues
                .entry(to)
                .or_default()
                .push((due, order, event));
        }
    }

    impl Link for Memory {
        fn send(&mut self, to: Peer, bytes: Vec<u8>) {
            self.post(to, Event::Message(self.me, bytes));
        }

        fn datagram(&mut self, to: Peer, bytes: Vec<u8>) {
            if self.dice.f() >= self.hub.loss {
                self.post(to, Event::Datagram(self.me, bytes));
            }
        }

        fn quality(&self, _peer: Peer) -> Option<Quality> {
            Some(Quality {
                ping: (self.hub.delay * 2 * 1000 / 60) as u16,
                direct: true,
            })
        }

        fn poll(&mut self) -> Option<Event> {
            let mut inner = self.hub.inner.lock().unwrap();
            let now = inner.now;
            let queue = inner.queues.get_mut(&self.me)?;
            let next = queue
                .iter()
                .enumerate()
                .filter(|(_, e)| e.0 <= now)
                .min_by_key(|(_, e)| e.1)?
                .0;
            Some(queue.remove(next).2)
        }
    }

    #[test]
    fn messages_arrive_late_and_in_order() {
        let hub = Hub::new(2, 0.0);
        let (mut host, mut player) = (hub.host(), hub.join());
        player.send(HOST, vec![1]);
        player.send(HOST, vec![2]);
        assert_eq!(host.poll(), None);
        hub.step();
        hub.step();
        assert_eq!(host.poll(), Some(Event::Joined(player.peer())));
        assert_eq!(host.poll(), Some(Event::Message(player.peer(), vec![1])));
        assert_eq!(host.poll(), Some(Event::Message(player.peer(), vec![2])));
        assert_eq!(host.poll(), None);
        assert_eq!(player.poll(), Some(Event::Joined(HOST)));
    }
}
