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
- **What is left:** `REMAINING.md` lists what of the original is not ported yet, the
  stand-ins and what is left out on purpose. Read it instead of surveying again, and
  keep it up to date as things are ported or gaps are found.
- **Code:** `src/assets/` holds the file-format readers; everything else in `src/` is
  the game. Each module's header comment says which part of the original it follows.
- **Maps:** the circuits and arenas of the port's own are files, read as the game
  runs (`src/map.rs`: a TOML file to a map, named for what it is known by, so
  `brick.toml` is `BRICK`). The game carries the eight of `maps/` (`map::stock`),
  which are all a demo or a test has unless `BRICK_MAPS` names a folder. A game
  being played has every map of its folder (`~/.brickracers/maps`), one named as
  one of the game's own in that one's place, and writes its own there to be
  changed: where there is none of that name, and over one nobody has changed
  since the game wrote it (`map::stock_up`, which keeps what it wrote in
  `.written`), so a map changed in `maps/` reaches a folder whose copy is
  untouched and never one that has been worked on; delete a file to have the
  game's again. A map's file is read again each time a race alone is loaded, so
  one being worked on is seen by restarting the race. One that is no map, or that
  no road can be built from, is said in the log and left out (`Map::read` says
  what is wrong; `Map::tried` builds it every way and refuses what panics).
  `maps/brick.toml` and `maps/yard.toml` say what a circuit's file and an
  arena's have in them. A map may say its `author`, which the race pages show
  under its name, the room after the circuit voted for, and a host tells its
  players with its list (`Circuit::credit`).
- **Kept between sessions:** everything is in one folder, `~/.brickracers`
  (`src/home.rs`; `BRICK_HOME` says another): `settings`, `garage`, `progress`,
  `ghosts/` and `maps/`. What an earlier game left loose in the home folder
  (`~/.brick_racers_settings` and the rest) is moved in as a game that keeps
  things starts (`home::gather`).
- **Our own circuits:** `src/track.rs` builds a map's road (`track::Road`) and
  what it has besides (byways, infields, speed pads and banked
  corners, each so far round the lap). Each road is a list of corners
  (`track::cornered`: x, height, z and the radius it is rounded to, with straights
  between and the heights eased from one to the next), narrow, and folded in on
  itself so that it is never straight or level for long and one stretch runs
  beside, over or under another: stretches side by side are two barriers apart, and
  one over another is `DECK` above it. No corner is tighter than a car going flat
  out can be slid round: a hairpin is eighteen or twenty to its middle (seventeen
  on the gauntlet), and a test drives every circuit that way
  (`kart`'s `every_corner_of_the_built_in_circuits_is_slid_round_flat_out`). The
  figure of eight also crosses itself on
  the level (`Track::crossings`), where neither road has a barrier or a verge across
  the other. So that little is seen from a car but the road it is on and the
  road it will be on next, a high wall stands round the outside of each corner,
  with arrows on it the way the corner goes, and a low one inside each of the
  tighter corners that are on the ground, to be seen over (`Track::screens`,
  which a car can reach and which are solid); road that is off the ground has a
  parapet over its barrier; and `src/roadside.rs` (the port's own) stands a wall
  half way between any two stretches that run beside each other on the level,
  high but for the last of the way into a hairpin and the first of the way out
  of it, and a hedge along the outside of a stretch that has nothing beside it,
  low on the inside of a bend, neither of them solid. None of these stands near
  the level crossing, across a byway's mouth or an infield, or where a hazard has
  something standing (`Track::clearings`), and nothing is scattered where a wall
  is low. What is scattered about is trees, with a brick only well away from the
  road. A test says how much is in sight from each place on a lap of the road
  elsewhere and of the road next (`roadside`'s
  `little_of_the_rest_of_the_road_is_seen_from_a_car`). A curse is left half
  way round a lap, well on from where it is set off and all on one side of the
  road (`gauntlet`'s `Curse`). The gates are
  every thirty-odd units, and neither they nor the lap zones are where a car on
  another stretch could reach them; a car that leaves its stretch is looked for on
  the others (`Track::strays`). The distances in `Extras` and those the hazards
  stand at are measured round the lap, and want measuring again when a road is
  changed. The computer's cars take the byways
  (`kart::Ai::byway`), `items::laid` puts the bricks where those features are,
  `src/gauntlet.rs` stands the hazards of the game's that a map asks for
  (`gauntlet::Stood`, one of each at the most), `src/landmarks.rs` builds the
  scenery a map asks for (the helter skelter's tower, stand, gantry, pond and
  rocks) and a map says what its walls are built of (`roadside::Look`). They have no picture for the race display's map, so one
  is drawn of the road (`hud::original::sketch`).
- **Arenas:** `src/arena.rs` (the port's own) builds a map that has a floor and
  no road: four places to fight in
  (the yard, the crossroads, the ring and the town), for a battle or a game
  of tag online and offered nowhere else: the room's vote lists them only when the
  host's mode is one of those (`frontend`'s `offered`), and the host runs a race
  voted onto one on the first circuit (`Rules::fitted`). An arena is a walled floor
  with no road (`Track::arena`), built of blocks, ramps and decks that are driven
  on and under (`arena::Plan`): the yard is three ways end to end with a bridge
  over the middle one and catwalks from it to its buildings' roofs, each with a
  ramp up from its end; the crossroads has a flyover across its junction, a deck
  across that climbed from east and west, and a pocket at the end of two arms;
  the ring a hill with a ramp up each side and a gallery against the wall north
  and south; and the town a street with a two-floored house at each end, a bus
  and a truck between them and a footbridge over those from the one house's
  upper floor to the other's. There
  are no gates, lap zones or finishing line, and the cars start apart, facing
  the middle.
  Its line (`arena::Plan::line`) is only where bricks are laid and where the
  computer's cars wander when they have nobody in sight to go for (`kart::Ai::quarry`:
  the nearest car, when they hold a power-up or are "it"). Each has a hazard or
  two of the game's (`gauntlet::load`), and its map is of its wall and blocks.
  A mirrored race turns the plan over (`Plan::mirrored`).
- **Built cars:** `src/build.rs` has the rules of putting bricks on a chassis, the
  bytes a car is saved as and the model made of one, and a racer's minifigure;
  `src/assets/leb.rs`, `gcb.rs` and `lrs.rs` read the bricks, the minifigure's heads
  and saved racers. The build menu's screens are `src/frontend/workshop.rs`, and
  `src/garage.rs` keeps the racers built. The racer the garage shows is the one the
  player races as alone; online it is one of the choices of who to race as, and goes
  to every game in the session whole (`net::protocol::Ride`), each making the car of
  it afresh. What to race as is chosen in a session's room, which has the garage
  off it ("build") and shows everyone's minifigure (`src/frontend/portraits.rs`).
- **Keys and pads:** `src/input.rs` has what the keys and a gamepad's buttons are
  bound to, which the race reads (`input::Actions`) instead of the keys themselves;
  the menus read keys, and a pad's buttons are passed on to them as keys. The
  bindings are changed on the options' controls page and kept with the settings.
  An axis can be bound to the accelerator and the brake (`input::Bound::Axis`; a
  pad's triggers to begin with), and from the countdown to the finish a pad hums
  with the engine (`input::engine_hum`: a shake that swells and dies away at the
  period of the original's sine, which a pad's motors can't play outright, and a
  steady one once that is too quick to play a frame at a time). No pad has been
  tried on any of this.
- **What has been won:** `src/progress.rs` keeps the circuits opened, the part sets
  and minifigure parts won and the records beaten; a racer's trophies are in its own
  record (`assets::lrs`). The build menu offers only what has been won. The options' "reset
  progress" (the port's own, `Page::Forget`) asks, and on "yes" shuts every circuit
  but the first, takes back the part sets, the records and every racer's trophies
  (`Progress::forget`); racers built and best time-race runs are kept. A circuit
  raced to the end has its film, and then a page of the menu says what was won
  (`frontend`'s `Page::Award`, the port's own).
- **Films:** `src/film.rs` plays the films of `/MENUDATA` (a `.CDB` of what begins
  and ends when, a `.CEB` of what that sets off, and world files of models) with
  the scenery's models and animations. The four for the places of a circuit are
  shown, and after a circuit won for the first time the film for its part set: the
  champion beside their car, or Rocket Racer's and then the credits, which the
  options have too. Every record beaten has Veronica Voltage's film, every
  circuit race begins with its circuit's. The game opens straight on its main
  menu, and is meant to: the notice the original opens on (`LEGAL`) is shown
  only when `BRICK_FILM` asks for it, and the two videos the original plays
  first (`HVSCmp.avi`, `introcmp.avi`, Indeo 5) are not to be ported. A film's
  lights light the models that have normals (`src/lighting.rs`: `Lights` is the
  scene's, `Lit` a mesh they light); only the films' models have any, and a race
  has no lights, so everything else keeps the colours it was made with. A film's
  changes of colour and sprays of particles are played; none has streamed sound.
  The view of a circuit on the race pages (`SINGRACE/PST.CDB`, one of these with
  a frame to each circuit) is `src/frontend/circuit.rs`'s. While a film plays the
  menus neither draw nor take keys (`film::Showing`). Online, the film for first
  place is shown about whoever won a race, before the room shows its results.
- **Minifigures standing:** `build::figure` makes one sitting, for a car, or
  standing, for the films and for the main menu, where the champion of the last
  circuit opened stands (`src/frontend/mascot.rs`), drawn onto a picture by a
  camera of its own, as the room's portraits are.
- **Models on the menus:** each is drawn onto a picture by a camera of its own, on
  a render layer of its own: `portraits.rs` (7), `mascot.rs` (8), `circuit.rs` (9,
  the race pages' view of the circuit, with the race's mascot stood in it and the
  frame's lights on it; the time race page's mascot is Veronica Voltage),
  `parts.rs` (10, the driver page's parts), `licence.rs` (11, the licence's
  photograph), `carousel.rs` (12, the bricks page's row of bricks, which slides from one
  brick to the next) and `stage.rs` (13, the build menu's sets, each in its frame:
  the racer and car of the garage and the racer page in `RS_SET`, the driver
  being dressed on the driver page in `CB_SET`, and the car of the car page and
  of the page bricks are placed on in `GARAGE`, which `workshop::show` stands
  there and turns), all in `src/frontend/`. The page
  bricks are placed on shows the original's help for what the pointer rests on
  (`workshop::Tip`), and what moves there (the view turning and rising, the ghost
  bricks, a brick dropping on) is `workshop::Motion`, stepped by `workshop::tick`. A query for the screen's own
  `Camera3d` must leave out those with `RenderLayers`, or it stops matching one
  camera once a second is there.
- **The idle demo:** `src/frontend/idle.rs`. The main menu left for sixty seconds
  goes to a race of the first circuit with the computer driving every car and
  "DEMO" flashing over it (`Idle::running` makes `Race::demo`); any key ends it.
  Keys on the menu don't put the wait back, as they don't in the original; leaving
  the page does. The settings are put back as they were afterwards.
- **Language:** `Settings::language` (an index into `assets::font::LANGUAGES`,
  picked on the options' language page) is the folder words come from:
  `/MENUDATA/<lang>` for the menus, `/GAMEDATA/COMMON/<lang>` for the race display
  and the loading screen, `<film>/<LANG>.SRF` for a film's words
  (`Film::load_in`). Nine are offered; Finnish is in the archive and not offered,
  as in the original. The token reader takes strings as Latin-1. Drivers are
  called what that language's `DRIVERS.SRF` calls them (`roster::name`); the
  port's own pages are English whatever is chosen.
- **The build menu's other pages:** the licence picks the face its driver pulls
  (`Cosmetics::expression`, worn in the menus and not in a race) and takes the
  original's cheat codes as its name (`src/cheats.rs`: they last the session, a
  circuit or time race clears them, and online only the three that change a
  car's looks apply). A face's material for a look the material files don't
  define is its plain one with the look's picture (`world::Library::material`).
  The licence page's layout places everything from the corner of the licence
  itself (`workshop::on_licence`), and shows the driver in its photograph only.
  Leaving a page with something changed asks first (`Page::Scrap`). The garage's
  test drive is the race of folder `TEST`, alone and with no lights to wait
  for. That circuit has no recorded route, so `world::load_in` gives it a ring
  for a road, a stand-in: the car starts on the circuit's one grid place and is
  driven where the player likes, but the computer (a demo's driver too) follows
  the ring and not the road, and nothing ends the drive but leaving it. A warp
  there takes the car nowhere, as the original's does without checkpoints
  (`Track::unrouted`).
- **Loading:** `src/loading.rs` shows the race's `LOADSCRN.LSB` picture and its
  ticks (`LoadingScreen`) while a race is loaded behind it, a step to a frame
  (`loading::Step`, `STEPS`), the ticks filling in as the steps are done. A race
  come to without the screen takes whatever steps are left as it is entered.
- **The car's body:** `src/physics.rs` follows `RacerCarBody` and
  `RacerRigidBody`: angular momentum and its impulses (`physics::Rigid`, sent
  online in `net::state::State`), with a mass and centre of mass (`Kart::mass`,
  `centre`) that a built car takes from its bricks (`build::Car::weight`). A car
  on its wheels is set a ride height above the ground (`RIDE_HEIGHT`); one
  floating on a turbo or a magnet is the original's slide body
  (`physics::probe_hover`): held up over the ground under its two axles, its nose
  lifted as it leaves the road and left so until it is back on its wheels.
- **Ribbons:** `src/beams.rs` draws the lightning's bolt, the hook's rope and the
  streaks behind missiles and cannon balls (`BeamMesh`, `RaceTrailManager`) from
  the `Action`s in the world; `hazards::hazard_looks` draws the crane's shadow,
  laid on the road as the original's decals are (`Collision::decal`), and the
  ghost's after-images. A hook that lets go winds its rope in, and leaves a
  puff where it let go: a picture that faces the camera and plays a track of
  the power-ups' material animation (`world::hook_puff`, an emitter of the one
  particle; `particles::EmitterDef::billboard`). Skid marks are `kart_effects`':
  from whichever wheels the original marks with, each piece cut to the road as
  the crane's shadow is. A car's shadow is the same kind of thing
  (`kart_effects::shadows`): the car's own outline seen from above
  (`world::KartModel::silhouette`), laid on the road under it. An explosion on
  the road leaves a scar that grows with it and fades, and a shot that strikes a
  car throws bricks off it (`item_models::aftermath`, from what
  `Action::Explosion` says of itself).
- **The driver:** `src/driver.rs` has each car's driver lean with the steering,
  look at cars alongside and behind, look back to reverse, start at a knock and
  end the race glad or not, on the skeleton and the animation every driver
  shares (`PELVIS`). `kart::spawn_karts` seats the rigged figure
  (`world::KartModel::figure`); ghosts and the cars of the films and the build
  menu keep the figure at rest.
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
  (`net::client`), so that what is seen and what is bumped into agree. How a race
  online is won is the host's to say on "rules" (`menu::Mode`, sent in
  `net::protocol::Rules`): a race, elimination, or one of the two of `src/net/modes.rs`
  that laps don't end, a battle (three lives, a spin or a blow into the air takes
  one, the last car left wins) and a game of tag (the car that is "it" runs its
  clock down until it bumps another; the first clock out ends it). What a car has
  left is `Kart::stake` and `Kart::it`, which go to the players with its standing
  and are shown where the display counts laps. A session's "series" scores any of
  them by place, race after race. A race on one of the port's own maps is run on
  the host's: its file goes to the players with the start (`ToPlayer::Start`,
  `Circuits::lend`) and is raced whatever their own games have of that name. The
  circuits there are to vote for are the host's: it tells each player its list
  as they join (`ToPlayer::Circuits`), a player's vote page shows that list and
  none of their own that the host hasn't (`frontend`'s `offered`), and a map
  only the host has is there by name (`Circuits::promise`) until a start brings
  it. The drop-in and the countdown wait for every player's game to say it has
  the race loaded (`ToHost::Loaded`, `host`'s `LOAD_WAIT` at the longest).
  The garage's test drive, taken from a session's room, is driven alone as it
  is offline (`net::Apart` keeps the session while `Role` says offline); a
  host's start fetches a player from it, and a host on it starts nothing.
- **Lobby server:** `crates/lobby` (with `crates/lobby-api`, which the game shares). It
  only lists sessions. The root `Dockerfile` builds its image and nothing of the game.

## Porting rules

- Read the original before writing. Find the function in the decompilation, take its
  constants and its order of operations, and name the original in the module comment.
- If the original does something the port has no feature for yet, build the feature.
  Don't leave a stand-in without saying so.
- What the port adds to the original (replays, photo mode, reversed circuits, the brick
  rules, elimination, quick steering, faster cars, the video options, online play and
  its battle and tag) is off until asked for, and says in its
  module comment that it is the port's own.
- Units: the game's are Z-up; ours are Y-up, with one game unit `physics::UNIT` of
  ours. Use `scenery::to_world` for positions.
- Rotations: the game multiplies vectors from the other side, so its quaternions must
  be conjugated (`assets::adb::turn`, `opponent::facing`). Getting this wrong shows up
  as doors opening backwards or wheels sitting beside their car.
- Comments say what the code does or why, as the surrounding code does. No history.
- No circuit's event table has look targets (0x55), external forces (0x59), event
  links (0x39), material animations (0x29) or changes of colour that name a model;
  `events.rs` doesn't read them, and a test says so.
- Not wanted, so not to be ported or listed as missing: the original's two-player
  split screen (online play has its place in the menus), and anything shown before
  the main menu as the game opens.

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
| `BRICK_RACE=RACEC0R0` | which race folder to load, or which map by its file's name (`TEST` is the garage's test drive; `BRICK`, `FIGURE8`, `GAUNTLET` and `HELTER` are the game's own circuits, `YARD`, `CROSSROADS`, `RING` and `TOWN` its arenas, where a demo's own car stays on the grid unless `BRICK_KEYS` drives it; the gauntlet's hazards are set off by events 101 to 119, see `src/gauntlet.rs`) |
| `BRICK_CAM=x,y,z,tx,ty,tz` | fixed camera, in the game's coordinates |
| `BRICK_VIEW=back,up,right` | camera placed relative to the player's car |
| `BRICK_POWER=green2@4,red0@6` | power-ups the player fires (colour, level, time) |
| `BRICK_EVENTS=18@3` | circuit events to set off, and when |
| `BRICK_KEYS=Escape@4,Down@4.5,E@6+0.5` | keys to press, when, and for how long held (a demo's keys are bound as a new game's are: `W` or `Up` is the accelerator; `P` is photo mode; `R` at the finish is the replay; where bricks are placed `I`, `J`, `K` and `L` move the brick, `R` turns it, `Enter` adds it, which takes most of a second as it drops onto the car, and only where it fits, which on one of the game's own cars is nowhere until `Back` has taken a brick off, `Tab` and `T` are the next brick and set, `Left` and `Right` turn the car, `Up` and `Down` raise and lower the view and `C` puts it back; on the driver page "done" waits a second or two for the driver's move before the page changes) |
| `BRICK_HOME=<folder>` | where everything kept is (default `~/.brickracers`); each of the next has its own place in it |
| `BRICK_MAPS=<folder>` | a folder of maps (default `~/.brickracers/maps`; a demo without it has the game's own and no others, and a demo writes nothing there). `BRICK_MAPS=maps` races the files of the repo as they are, without building again |
| `BRICK_SETTINGS=<file>` | where settings are kept (default `~/.brickracers/settings`; demos neither read nor write it) |
| `BRICK_START=1` | keep the drop-in and countdown (demos skip them) |
| `BRICK_LAPS=1` | race length |
| `BRICK_SERIES=0` | race that circuit's races as a circuit race |
| `BRICK_TIME=1` | time race |
| `BRICK_MENU=race\|circuit\|time\|options\|game\|audio\|video\|extras\|controls\|award\|online\|host\|join\|garage\|racer\|driver\|licence\|car\|bricks\|language` | menu page to open on; `language` is the options' page, where `Right` and `Left` change it (a demo reads no settings, so that is how one is put in another language); the five before it are of the build menu, with the racer `BRICK_RACER` names on the bench; `award` is a circuit won, as it is the first time the first is: its film (eleven and a half seconds), then the page that says what was won |
| `BRICK_GARAGE=<file>` | where the racers built are kept (default `~/.brickracers/garage`; a demo without it has the game's 24 quick-build racers for a garage, and keeps nothing) |
| `BRICK_FILM=C_AWARD1` | a film to show as the menu opens, by its folder in `/MENUDATA` (`C_AWARD1` to `C_AWARD4` are the circuit's places, `WINCAR` a champion's car set won, `WINCAR:c3` being the fourth circuit's, `WINRRCAR` Rocket Racer's, which is thirty-seven seconds, `WINVVCAR` Veronica Voltage's, `CIRCUIT1` to `CIRCUIT7` those before each circuit, `LEGAL` the opening notice and `CREDITS` the credits, which are two minutes; a key ends it after a second) |
| `BRICK_PROGRESS=<file>` | where what has been won is kept (default `~/.brickracers/progress`; a demo without it has everything won, and keeps nothing; a file that isn't there yet is a game with nothing won) |
| `BRICK_CHEATS=FSTFRWRD,NWHLS` | the licence's cheat codes, as if each were typed as its name (`NSLWJ`, `FLYSKYHGH`, `PGLLRD`, `PGLLYLL`, `PGLLGRN`, `LNFRRRM`, `RPCRNLY`, `MXPMX`, `FSTFRWRD`, `NWHLS`, `NCHSSS`, `NDRVR`; `NMRCHTS` clears them) |
| `BRICK_LOADING=4` | begin on the loading screen and hold it that many seconds of the real clock, its ticks filling in (demos otherwise skip it); `0` shows the load itself, as long as it takes |
| `BRICK_IDLE=2` | how many seconds the main menu waits before its demo race (sixty without it), and lets a demo run go to one, which it otherwise never does |
| `BRICK_RACER=4` | which of the garage's racers the player races as, counted from one |
| `BRICK_MIRROR=1`, `BRICK_REVERSE=1` | race the circuit mirrored, or the other way round |
| `BRICK_ELIMINATION=1` | the last car goes out each lap |
| `BRICK_SPEED=2` | how many times as fast as the original's the cars are, the computer's too (1 to 2 by quarters; `Kart::pace`; online it is the host's that counts) |
| `BRICK_BRICKS=red\|yellow\|blue\|green\|none\|random` | what the circuit's bricks are made |
| `BRICK_OPPONENTS=1` | how many of the computer's cars race |
| `BRICK_PHOTOS=<folder>` | where photo mode saves (default `screenshots/`) |
| `BRICK_GHOSTS=<folder>` | where best time-race runs are kept (default `~/.brickracers/ghosts`) |
| `BRICK_SOUND=1` | let the demo be heard (see below) |
| `BRICK_NET=host:2` | host a session and start its race, without a vote, once that many players are in it (the host is one); `host:9` never fills, and leaves the session in its room |
| `BRICK_NET=join:Demo` | join the session of that title once the lobby lists it |
| `BRICK_MODE=battle\|tag\|elimination` | how the race a demo hosts is won (`BRICK_NET=host:1` hosts one against the computer's cars alone) |
| `BRICK_SESSION`, `BRICK_NAME`, `BRICK_CAR`, `BRICK_PASSWORD` | the title hosted under (default `Demo`), the player's name, who they race as (a code from `roster::NAMES`, such as `PH`, or a number for that one of the garage's racers), the password set or given |
| `BRICK_LOBBY=http://localhost:18096` | which lobby to use (default the one on the homelab) |
| `RUST_LOG=blockracers::hazards=debug` | per-module logging |

An online demo is two runs at once, a host and a joiner, each with its own
`BRICK_DEMO`; both wait at the menu until the session's race begins, so allow ten
seconds or so before anything is on the road. Online the demo's cars are driven by
`BRICK_KEYS` (`W@10+12`), not by the computer. A demo can't type, so the pages
with something to type into are reached by `BRICK_MENU` and `BRICK_NET` and not
through the menus; in the room, `Enter` is on "ready" to begin with. After a race
the room opens on its results, where `Enter` is on "OK". The room is a column of
rows down its left, in the order `Down` goes through them ("ready", the vote, what
to race as, "build a racer", then the host's "race rules", "session" and "start
now"), with everyone's minifigure on its right and the next race said under them. The room's other pages
(the last race, and the host's page for the password, the player limit and putting
a player out) are reached with `BRICK_KEYS`. In a race online the question `Escape`
asks opens on "no": `Escape@13,Up@13.4,Enter@13.8` gives the race up. In the room
while a race is on, the row for it is the first under "build a racer". Two `Down`s from
"ready" is what to race as, which `Left` and `Right` change, and one `Down` is
"vote for a circuit", the page the next race's circuit is voted for on (`Enter` on a
circuit votes for it; the rest of how the race is run is the host's, on "rules"); "build a racer", under what to race as, opens the garage, and `Escape` there comes back.

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
- **Give a run a hard stop.** One demo sat for eighteen minutes without quitting
  while the renderer was logging errors. Start it in the background and kill it if
  it outlives its time by more than a few seconds.
- **A build folder to each checkout.** Two checkouts building into one `target/`
  overwrite each other's test binary, and a test run then tests the other's code.
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
