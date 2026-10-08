# What is left of the original

What the port doesn't yet do that LEGO Racers does, so that it needn't be worked out
again. Take a thing off this list when it is ported, and put one on when a stand-in
is left or a gap is found.

Last gone through on 2026-10-08, by reading each module's header and looking for the
decompilation's classes and screens in `src/`. That is a survey and not a function by
function audit: a feature that is ported but differs in detail won't have been caught.

## Not ported

- **Drivers' names in other languages.** The menus, the race display, the loading
  screen, circuits' names and films' words are in the language chosen; the drivers'
  names are the port's own table (`roster::NAMES`), in English. The original's are
  in each language's `DRIVERS.SRF`.
- **The build menu's help.** `CARBUILD.SRF` has words for the bricks page that the
  port doesn't show; it has its own lines saying what the keys do.
- **The garage's frame.** The showcase the racer and car stand in has no frame
  round it (`bluebox` and the corner pictures of `GARAGE.MIB`).
- **The row of bricks sliding.** The bricks page's row steps from one brick to the
  next; the original's slides (`MenuModelCarousel`).
- **The racer shown on the other build pages.** The garage and the racer page show
  the racer and car in the original's set (`frontend/stage.rs`); the driver,
  licence, car and bricks pages, and the question asked on leaving them, still
  show the car alone as before (`workshop::show`). What the original shows on each
  has not been gone through.

## Stand-ins

- **The licence photograph's camera** is placed by hand; the original's is a scene
  file's, which the port doesn't read for this screen (`src/frontend/licence.rs`).
- **Placing bricks** is by the keyboard, with keys of the port's own, the original
  being played with a pad (`src/frontend/workshop.rs`).
- **The loading screen's ticks.** The port loads in fewer parts and another order,
  so each step has the original's figure for the part most like it (`src/loading.rs`).
- **The engine's hum on a pad** is the nearest a pad's motors come to the original's
  force-feedback sine (`input::engine_hum`).
- **The idle demo's race** is picked by the clock; the original picks with a table
  of random numbers of its own (`g_randomTable`). It is run at the difficulty and
  car speed of the settings, which the original has no say in.
- **The car in the garage** is lifted by `physics::RIDE_HEIGHT`, by eye; where the
  original stands it has not been read.
- **The mascot in the circuit's view** is not lit by the frame's lights, though the
  scene is; nor is the racer of a film.

## Not checked

- No gamepad has been tried on any of the pad's code: bindings, axes, the shaking
  or the hum.
- The idle demo: a race of it run to the end, which should leave ten seconds after
  the finish; a mouse or pad button ending it; a wait of the whole sixty seconds.
- Another language in a race, on the loading screen, in a film's words and in the
  circuits' names: the files are read, and only the menus have been looked at.
- Whether the garage's figure plays its idle moves, the mascot in the circuit's view
  its own, and which way the row of bricks spins: stills don't show them, and the
  spin's direction is a guess.
- The mouse on the language page, the row of bricks and the garage's showcase.
- Drift dust and wheel spray can't be seen in a demo, whose driver never drifts or
  leaves the road.

## Left out on purpose

Not to be ported, and not to be listed as missing:

- The two-player split screen; the options' "player 2" controls button is dead for
  that reason. Online play has its place in the menus.
- Anything shown before the main menu as the game opens, the two videos among them.
- The original's save, load and memory card screens: the port keeps a garage and
  what has been won in files of its own.
- Streamed sound in films, which no film has, and the kinds of event no circuit's
  table has (look targets, external forces, event links, material animations).

## Gone through and found ported

All twenty hazard classes and every power-up action; the car's body, the driver's
animations, decals, shadows and trails; the films, the race display, circuit and
time races; the cheats, the mouse in the menus and force feedback; the idle demo
race, the language page, the mascot and lights of the circuit's view, the row of
bricks and the garage's racer and car.
