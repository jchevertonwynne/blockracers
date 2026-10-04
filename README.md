# Brick Racers

A port of LEGO Racers (1999) to Rust and [Bevy](https://bevyengine.org). It plays the
original game's circuits, cars, sounds and menus from the original game's own data
files, and adds a few things of its own, racing other people online among them.

Nothing of the original game is in this repository. You need your own copy.

## What you need from the original game

Two things, both from the `Game Files` folder of an installed copy of LEGO Racers for
Windows (English):

| File | What it is | Needed? |
|---|---|---|
| `LEGO.JAM` | the game's archive: circuits, cars, textures, sounds, menus | for the real game |
| `*.tun` (about fifty of them) | the music | only to hear music |

Put them where the game looks:

```
Lego_Racers_Win_Files_EN/
  Game Files/
    LEGO.JAM
    Circuit1.tun
    ...
```

or keep them anywhere and say where the archive is; the music is looked for beside it:

```sh
LEGO_JAM=/path/to/LEGO.JAM cargo run
```

That folder is git-ignored. Nothing else from the original is used: not `LEGORacers.exe`,
`GolDP.dll`, the `.avi` videos or the save file.

Without `LEGO.JAM` the game still starts, on a few brick-built circuits of its own with
brick-built cars, without the original's look or sound.

## Running it

You need [Rust](https://rustup.rs).

```sh
cargo run --release
```

| Keys | |
|---|---|
| `W` / `↑`, `S` / `↓` | accelerate, brake |
| `A` / `←`, `D` / `→` | steer |
| `Shift` | powerslide (with the accelerator held) |
| `Space` | use the power-up held; the horn without one |
| `C`, `V` | change the view; look behind |
| `Esc` | pause |

## Racing online

One player hosts from inside the game and the others join; nobody needs to forward a
port. Games find each other's sessions on a small lobby server and then connect to the
host directly. Everyone needs their own copy of the game data.

Online play is being built. Today it is reached only by environment variables, cars
and their collisions are shared but a joining player doesn't yet see bricks being taken
or power-ups in flight, and there are no menus for it:

```sh
BRICK_NET=host:2 BRICK_SESSION="Friday night" BRICK_NAME=Me cargo run --release    # host, starting once two are in
BRICK_NET=join:"Friday night" BRICK_NAME=You cargo run --release                   # join
```

## For people working on it

`AGENTS.md` describes the layout, the rules the port is written by, and the variables
that drive demo runs. The lobby server is `crates/lobby`:

```sh
cargo test                # the game
cargo test -p lobby       # the lobby server
```

How the original behaves is taken from the [isledecomp/racers](https://github.com/isledecomp/racers)
decompilation, which is read as a reference and is not part of this repository either.
