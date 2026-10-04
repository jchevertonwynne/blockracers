//! What the game and the lobby server say to each other. The port's own: the original
//! finds its opponents on the same machine.
//!
//! The lobby keeps a list of the sessions being hosted and nothing else. A host puts
//! its session on the list and keeps saying it is still there; a player reads the list
//! and dials the host's `endpoint` directly. A password, where a session has one, is
//! the host's to check when a player joins, so the lobby is only told it is `locked`.

use serde::{Deserialize, Serialize};

/// Games list and see only sessions of their own protocol.
pub const PROTOCOL: u32 = 1;

/// How often a host says its session is still there, and how long the lobby waits
/// without hearing before it takes the session off the list, in seconds.
pub const BEAT_EVERY: u64 = 10;
pub const GONE_AFTER: u64 = 30;

/// The most cars in a race, and so players in a session.
pub const MAX_PLAYERS: u8 = 6;
/// The longest a name may be, in bytes: the session's, the host's, the circuit's.
pub const MAX_NAME: usize = 32;
/// The longest an `endpoint` may be, in bytes.
pub const MAX_ENDPOINT: usize = 512;

/// A host putting its session on the list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Register {
    pub protocol: u32,
    pub name: String,
    /// The name of the player hosting.
    pub host: String,
    /// How to dial the host. The lobby passes it on without reading it.
    pub endpoint: String,
    pub locked: bool,
    pub max: u8,
    pub status: Status,
}

/// What changes about a session while it is listed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Status {
    pub players: u8,
    /// The circuit being raced or last raced; empty before the first vote.
    pub circuit: String,
    /// A race is on, so a player joining now waits in the room for the next.
    pub racing: bool,
}

/// The lobby's answer to `Register`. The `token` is what lets the host, and nobody
/// else, change the session or take it down.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Registered {
    pub id: String,
    pub token: String,
}

/// A session as the list shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub host: String,
    pub endpoint: String,
    pub locked: bool,
    pub max: u8,
    pub status: Status,
}
