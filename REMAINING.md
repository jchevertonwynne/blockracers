# What is left of the original

What the port doesn't yet do that LEGO Racers does, so that it needn't be worked out
again. Take a thing off this list when it is ported, and put one on when a stand-in
is left or a gap is found.

Last gone through on 2026-10-08, by reading each module's header and looking for the
decompilation's classes and screens in `src/`. That is a survey and not a function by
function audit: a feature that is ported but differs in detail won't have been caught.

## Not ported

- **The other views of a car being built.** Where bricks are placed the car is seen
  from one height and is at once as it is turned to. The original has two higher
  views of it (`CarPartPlacement::PitchViewStep`, the second and third of
  `g_carPartCameraMinPositions`) and turns smoothly from one view to the next
  (`UpdateViewRotation`, `UpdateViewPitch`).
- **The held brick's movements.** It does not bob over the car or drop onto it as
  it is put on (`UpdatePieceBob`, `UpdateCommitFeedback`).
- **The rest of the build page's help.** Help is shown for the things the port's
  page has. The original has help too for its pad of arrows that move the brick,
  for the camera and for two more things (strings 2, 6, 7 and 8 of `CARBUILD.SRF`),
  which the port's page has not got, being worked by the keyboard.
- **The driver's leaving move.** A driver being dressed makes a move as the page
  is done with (`EditDriverScreen::PlayExitAnimation`), and the page waits for it;
  here the page is left at once.

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
- **How far the camera stands back from a car being built** is by the car's length
  and width (`KartModel::outline`); the original's is by the radius of the whole
  car (`GetViewPosition`, `GetBoundsRadius`).
- **The build page's help** names the original's keys (the numeric keypad and the
  rest), which are not the port's; the port's own are in the lines the page has
  beside it.
- **Which move a driver makes when its legs are changed** is by which legs they
  are; the original picks one of the two with its table of random numbers.

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
- The mouse on the language page, the row of bricks and the garage's showcase, and
  the build page's help coming up under a pointer left on something: its timing is
  tested and its look was seen by showing it without a pointer.
- Drivers' names in a race in another language than English: the names are read
  and tested, and only the circuit page's was looked at, in English.
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
bricks and the garage's racer and car; drivers' names in each language, the build
page's help, the frames of the garage and the driver page, the row of bricks
sliding, the driver on its platform and the set a car is built in.
