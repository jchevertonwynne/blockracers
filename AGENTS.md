# Working on Brick Racers

A racing game in Rust and Bevy, based on LEGO Racers (1999): it loads the original
game's data and aims to behave, sound and look like it; the decompilation is the
reference for how. It is its own project, and not the LEGO Group's: it is called Brick
Racers, its package is `blockracers`, and nothing it says of itself claims otherwise.

## Where things are

- **Game data:** `Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM` (git-ignored; override
  with `BRICK_JAM`). Without it the game falls back to a built-in brick circuit, and the
  tests that need it pass silently.
- **Reference source:** the isledecomp/racers decompilation, checked out at
  `target/ref/racers` (`LEGORacers/src`, `common/`, `GolDP/`). It lives under `target/`,
  so **`cargo clean` deletes it** — don't run it without re-cloning afterwards.
- **Code:** `src/assets/` holds the file-format readers; everything else in `src/` is
  the game. Each module's header comment says which part of the original it follows.
- **Built cars:** `src/build.rs` has the rules of putting bricks on a chassis, the
  bytes a car is saved as and the model made of one, and a racer's minifigure;
  `src/assets/leb.rs`, `gcb.rs` and `lrs.rs` read the bricks, the minifigure's heads
  and saved racers. The build menu's screens are `src/frontend/workshop.rs`, and
  `src/garage.rs` keeps the racers built. The racer the garage shows is the one the
  player races as alone; online it is one of the choices of who to race as, and goes
  to every game in the session whole (`net::protocol::Ride`), each making the car of
  it afresh. What to race as is chosen in a session's room, which has the garage
  off it ("build") and shows everyone's minifigure (`src/frontend/portraits.rs`).
- **Online play:** `src/net/` (the port's own). The host's game runs the race; players'
  games drive their own car ahead of the host's word and are shown the rest. Online
  the race is stepped in `FixedUpdate` at 60 Hz by `net::plugin`, not per frame, and
  pause, photo mode and replays are off. Bricks, power-ups and the circuit's events
  are run only on the host, which tells the others what came of them (`net::scene`);
  a joiner's `TrackEvents` is `following` and starts nothing by itself. A player who
  gives a race up is back in the session's room, and their car is the computer's;
  from the room they can go back to it, and someone with no car in a race watches
  it (`net::Watching`: on their game the car followed carries `Player`). A joiner's own car
  is driven ahead of the host and the others are shown where they will be by then
  (`net::client`), so that what is seen and what is bumped into agree.
- **Lobby server:** `crates/lobby` (with `crates/lobby-api`, which the game shares). It
  only lists sessions. The root `Dockerfile` builds its image and nothing of the game.

## Porting rules

- Read the original before writing. Find the function in the decompilation, take its
  constants and its order of operations, and name the original in the module comment.
- If the original does something the port has no feature for yet, build the feature.
  Don't leave a stand-in without saying so.
- What the port adds to the original (replays, photo mode, reversed circuits, the brick
  rules, elimination, quick steering, the video options, online play) is off until asked for, and says in its
  module comment that it is the port's own.
- Units: the game's are Z-up; ours are Y-up, with one game unit `physics::UNIT` of
  ours. Use `scenery::to_world` for positions.
- Rotations: the game multiplies vectors from the other side, so its quaternions must
  be conjugated (`assets::adb::turn`, `opponent::facing`). Getting this wrong shows up
  as doors opening backwards or wheels sitting beside their car.
- Comments say what the code does or why, as the surrounding code does. No history.

## Running it

```sh
cargo build
cargo test           # the game's tests; add -p lobby for the lobby server's
cargo run            # the game, from the main menu
cargo run -p lobby -- -addr 127.0.0.1:18096    # a lobby to test against
cargo test real_endpoints -- --ignored         # two real endpoints; needs the internet
```

### Demo runs

`BRICK_DEMO=<seconds>:<png path>` goes straight into a race with the computer
driving, saves a screenshot at that time and quits. Add `:menu` to photograph the
menu instead. These combine with it:

| Variable | Does |
|---|---|
| `BRICK_RACE=RACEC0R0` | which race folder to load (`BRICK`, `FIGURE8` and `GAUNTLET` are the built-in circuits; the gauntlet's hazards are set off by events 101 to 119, see `src/gauntlet.rs`) |
| `BRICK_CAM=x,y,z,tx,ty,tz` | fixed camera, in the game's coordinates |
| `BRICK_VIEW=back,up,right` | camera placed relative to the player's car |
| `BRICK_POWER=green2@4,red0@6` | power-ups the player fires (colour, level, time) |
| `BRICK_EVENTS=18@3` | circuit events to set off, and when |
| `BRICK_KEYS=Escape@4,Down@4.5,E@6+0.5` | keys to press, when, and for how long held (`P` is photo mode; `R` at the finish is the replay; where bricks are placed the arrows move the brick, `R` turns it, `Enter` adds it, `Back` takes one off, `Tab` and `T` are the next brick and set, `Comma` and `Period` turn the car) |
| `BRICK_SETTINGS=<file>` | where settings are kept (default `~/.brick_racers_settings`; demos neither read nor write it) |
| `BRICK_START=1` | keep the drop-in and countdown (demos skip them) |
| `BRICK_LAPS=1` | race length |
| `BRICK_SERIES=0` | race that circuit's races as a circuit race |
| `BRICK_TIME=1` | time race |
| `BRICK_MENU=race\|circuit\|time\|options\|game\|audio\|video\|extras\|online\|host\|join\|garage\|racer\|driver\|licence\|car\|bricks` | menu page to open on; the last five are of the build menu, with the racer `BRICK_RACER` names on the bench |
| `BRICK_GARAGE=<file>` | where the racers built are kept (default `~/.brick_racers_garage`; a demo without it has the game's 24 quick-build racers for a garage, and keeps nothing) |
| `BRICK_RACER=4` | which of the garage's racers the player races as, counted from one |
| `BRICK_MIRROR=1`, `BRICK_REVERSE=1` | race the circuit mirrored, or the other way round |
| `BRICK_ELIMINATION=1` | the last car goes out each lap |
| `BRICK_BRICKS=red\|yellow\|blue\|green\|none\|random` | what the circuit's bricks are made |
| `BRICK_OPPONENTS=1` | how many of the computer's cars race |
| `BRICK_PHOTOS=<folder>` | where photo mode saves (default `screenshots/`) |
| `BRICK_GHOSTS=<folder>` | where best time-race runs are kept (default `~/.brick_racers_ghosts`) |
| `BRICK_SOUND=1` | let the demo be heard (see below) |
| `BRICK_NET=host:2` | host a session and start its race, without a vote, once that many players are in it (the host is one); `host:9` never fills, and leaves the session in its room |
| `BRICK_NET=join:Demo` | join the session of that title once the lobby lists it |
| `BRICK_SESSION`, `BRICK_NAME`, `BRICK_CAR`, `BRICK_PASSWORD` | the title hosted under (default `Demo`), the player's name, who they race as (a code from `roster::NAMES`, such as `PH`, or a number for that one of the garage's racers), the password set or given |
| `BRICK_LOBBY=http://localhost:18096` | which lobby to use (default the one on the homelab) |
| `RUST_LOG=blockracers::hazards=debug` | per-module logging |

An online demo is two runs at once, a host and a joiner, each with its own
`BRICK_DEMO`; both wait at the menu until the session's race begins, so allow ten
seconds or so before anything is on the road. Online the demo's cars are driven by
`BRICK_KEYS` (`W@10+12`), not by the computer. A demo can't type, so the pages
with something to type into are reached by `BRICK_MENU` and `BRICK_NET` and not
through the menus; in the room, `Enter` is on "ready" to begin with. After a race
the room opens on its results, where `Enter` is on "OK". The room's other pages
(the last race, and the host's page for the password, the player limit and putting
a player out) are reached with `BRICK_KEYS`. In a race online the question `Escape`
asks opens on "no": `Escape@13,Up@13.4,Enter@13.8` gives the race up. In the room
while a race is on, `Down` then `Enter` from "ready" goes to the race. `Up` from
"ready" is what to race as, which `Left` and `Right` change, and `Up` again is
"next race", the page the next race's circuit is voted for on (`Enter` on a
circuit votes for it; the rest of how the race is run is the host's, on "rules"); "build", the last of
the buttons under "ready", opens the garage, and `Escape` there comes back.

## Being a good guest on the user's machine

The game opens a window and plays audio on the machine the user is sitting at.

- **Keep it quiet.** Demo runs are silent by default. Only set `BRICK_SOUND=1` when
  the thing being tested is the sound itself, and say so first.
- **Never leave a run that can't end.** A demo quits by itself; anything that pauses
  or waits for a key must be timed on the real clock (`Time<Real>`), not game time,
  or it will sit in front of the user until they dismiss it.
- **Keep runs short.** Pick the earliest time that shows the thing. A full race takes
  minutes; use `BRICK_LAPS=1` or fire the event directly.
- **Screenshots come back black (56,997 bytes) when the window is covered.** That is
  the user working in another window, not a rendering bug. Retry, and get on with
  something that doesn't need the screen in the meantime.
- **Don't trust a screenshot you didn't just take.** Check the file's time, and look
  at it before describing what it shows.

## Checking work

- A change to how something looks is checked by looking: take the screenshot and read
  it. A change to rules or parsing gets a test against the real data.
- Say plainly what was seen on screen, what is covered only by tests, and what was
  not checked at all.
- The demo's computer-driven player never drifts or leaves the road, so anything
  behind those (drift dust, wheel spray) can't be seen in a demo; say so.

## Git

- Work is on `master`. Commit only when asked.
- The game data and `target/` are ignored; nothing from the original game is committed.
