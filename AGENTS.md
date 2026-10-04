# Working on Brick Racers

A Rust/Bevy port of LEGO Racers that loads the original game's data. The aim is to
behave, sound and look like the original; the decompilation is the reference for how.

## Where things are

- **Game data:** `Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM` (git-ignored; override
  with `LEGO_JAM`). Without it the game falls back to a built-in brick circuit, and the
  tests that need it pass silently.
- **Reference source:** the isledecomp/racers decompilation, checked out at
  `target/ref/racers` (`LEGORacers/src`, `common/`, `GolDP/`). It lives under `target/`,
  so **`cargo clean` deletes it** — don't run it without re-cloning afterwards.
- **Code:** `src/assets/` holds the file-format readers; everything else in `src/` is
  the game. Each module's header comment says which part of the original it follows.

## Porting rules

- Read the original before writing. Find the function in the decompilation, take its
  constants and its order of operations, and name the original in the module comment.
- If the original does something the port has no feature for yet, build the feature.
  Don't leave a stand-in without saying so.
- What the port adds to the original (replays, photo mode, reversed circuits, the brick
  rules, elimination, the video options) is off until asked for, and says in its
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
cargo test
cargo run            # the game, from the main menu
```

### Demo runs

`BRICK_DEMO=<seconds>:<png path>` goes straight into a race with the computer
driving, saves a screenshot at that time and quits. Add `:menu` to photograph the
menu instead. These combine with it:

| Variable | Does |
|---|---|
| `LEGO_RACE=RACEC0R0` | which race folder to load (`BRICK` and `FIGURE8` are the built-in circuits) |
| `BRICK_CAM=x,y,z,tx,ty,tz` | fixed camera, in the game's coordinates |
| `BRICK_VIEW=back,up,right` | camera placed relative to the player's car |
| `BRICK_POWER=green2@4,red0@6` | power-ups the player fires (colour, level, time) |
| `BRICK_EVENTS=18@3` | circuit events to set off, and when |
| `BRICK_KEYS=Escape@4,Down@4.5` | keys to press, and when (`P` is photo mode; `R` at the finish is the replay) |
| `BRICK_START=1` | keep the drop-in and countdown (demos skip them) |
| `BRICK_LAPS=1` | race length |
| `BRICK_SERIES=0` | race that circuit's races as a circuit race |
| `BRICK_TIME=1` | time race |
| `BRICK_MENU=race\|circuit\|time\|options\|game\|audio\|video\|extras` | menu page to open on |
| `BRICK_MIRROR=1`, `BRICK_REVERSE=1` | race the circuit mirrored, or the other way round |
| `BRICK_ELIMINATION=1` | the last car goes out each lap |
| `BRICK_BRICKS=red\|yellow\|blue\|green\|none` | what the circuit's bricks are made |
| `BRICK_OPPONENTS=1` | how many of the computer's cars race |
| `BRICK_PHOTOS=<folder>` | where photo mode saves (default `screenshots/`) |
| `BRICK_GHOSTS=<folder>` | where best time-race runs are kept (default `~/.brick_racers_ghosts`) |
| `BRICK_SOUND=1` | let the demo be heard (see below) |
| `RUST_LOG=legoracers::hazards=debug` | per-module logging |

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
