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

Choose **Online race** on the main menu. Give yourself a name, then either host a
race (a title, and a password if you want one) or pick one from the list to join. A
session waits in its room between races, where everyone says how they would have the
next one run: the circuit, laps, computer cars, and the port's own ways of racing. When
everyone is ready, or half a minute after the first is, the circuit is drawn from those
asked for and the rest goes to the majority, the host settling a tie.

The host's game runs the race, and everyone else is shown it: the cars, the bricks,
the power-ups and what the circuit does. Your own car answers at once and is put right
by the host when the two disagree, which you may feel as a nudge when cars touch.

## For people working on it

`AGENTS.md` describes the layout, the rules the port is written by, and the variables
that drive demo runs. The lobby server is `crates/lobby`:

```sh
cargo test                # the game
cargo test -p lobby       # the lobby server
```

How the original behaves is taken from the [isledecomp/racers](https://github.com/isledecomp/racers)
decompilation, which is read as a reference and is not part of this repository either.
