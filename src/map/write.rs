//! A map written out as the file it would be read from, for the builder
//! (`crate::builder`), which changes a map as the game runs and keeps it. What is
//! written reads back as the same map (`Map::read`), and is laid out to be changed
//! by hand afterwards: a thing to a line, with a word over each kind of thing.

use super::{COLOURS, Map};
use crate::arena::{Hue, Plan};
use crate::items::Power;
use crate::landmarks::Landmark;
use crate::roadside::Look;
use crate::track::Road;
use bevy::prelude::*;
use std::fmt::Write;

/// Numbers as a file has them, with commas between, and nought without a sign.
fn listed(numbers: &[f32]) -> String {
    let numbers: Vec<String> = numbers
        .iter()
        .map(|number| (number + 0.0).to_string())
        .collect();
    format!("[{}]", numbers.join(", "))
}

/// Words as a file has them, between quotation marks.
fn quoted(words: &str) -> String {
    let mut quoted = String::from('"');
    for letter in words.chars() {
        match letter {
            '"' | '\\' => quoted.extend(['\\', letter]),
            letter if letter.is_control() => quoted += &format!("\\u{:04X}", letter as u32),
            letter => quoted.push(letter),
        }
    }
    quoted + "\""
}

/// A colour as a file has it: its name, if it is one of those with a name.
pub fn colour(colour: Color) -> String {
    match COLOURS.iter().find(|named| named.1 == colour) {
        Some(named) => quoted(named.0),
        None => {
            let mixed = colour.to_srgba();
            listed(&[mixed.red, mixed.green, mixed.blue])
        }
    }
}

/// The point of the compass a way along the ground is.
pub fn compass([x, z]: [f32; 2]) -> &'static str {
    match (x.abs() > z.abs(), x > 0.0, z > 0.0) {
        (true, true, _) => "east",
        (true, false, _) => "west",
        (false, _, true) => "south",
        (false, _, false) => "north",
    }
}

/// A kind of thing, one to a line, under what it is.
fn section(text: &mut String, about: &str, name: &str, lines: Vec<String>) {
    if !lines.is_empty() {
        let _ = write!(text, "\n# {about}\n{name} = [\n");
        for line in lines {
            let _ = writeln!(text, "    {line},");
        }
        *text += "]\n";
    }
}

impl Map {
    /// The file this map would be read from.
    pub fn written(&self) -> String {
        let mut text = format!("name = {}\n", quoted(&self.name));
        if !self.author.is_empty() {
            let _ = writeln!(text, "author = {}", quoted(&self.author));
        }
        match &self.plan {
            Some(plan) => floor(&mut text, plan),
            None => road(&mut text, &self.road, &self.look),
        }
        let hazards = self.hazards.iter().map(|stood| {
            let mut line = format!(
                "{{ kind = {}",
                quoted(&format!("{:?}", stood.kind).to_lowercase())
            );
            if let Some(at) = stood.at {
                let _ = write!(line, ", at = {at}");
            }
            if stood.right != 0.0 {
                let _ = write!(line, ", right = {}", stood.right);
            }
            if let Some(place) = stood.place {
                let _ = write!(line, ", place = {}", listed(&place));
            }
            if stood.heading != 0.0 {
                let _ = write!(line, ", heading = {}", stood.heading);
            }
            for (name, value) in [
                ("reach", stood.reach),
                ("apart", stood.apart),
                ("out", stood.out),
            ] {
                if let Some(value) = value {
                    let _ = write!(line, ", {name} = {value}");
                }
            }
            if let Some(drops) = stood.drops {
                let drops: Vec<String> = drops.iter().map(|drop| listed(drop)).collect();
                let _ = write!(line, ", drops = [{}]", drops.join(", "));
            }
            line + " }"
        });
        section(
            &mut text,
            "The game's hazards stood about it, one of each at the most.",
            "hazards",
            hazards.collect(),
        );
        section(
            &mut text,
            "What stands about it to be looked at.",
            "scenery",
            self.scenery.iter().map(landmark).collect(),
        );
        text
    }
}

fn landmark(landmark: &Landmark) -> String {
    let kind = format!("{:?}", landmark.kind).to_lowercase();
    let mut line = format!(
        "{{ kind = {}, place = {}",
        quoted(&kind),
        listed(&landmark.place)
    );
    for (name, value, usual) in [
        ("heading", landmark.heading, 0.0),
        ("length", landmark.length, Landmark::length()),
        ("radius", landmark.radius, Landmark::radius()),
    ] {
        if value != usual {
            let _ = write!(line, ", {name} = {value}");
        }
    }
    line + " }"
}

/// A circuit's road, and what its walls are built of.
fn road(text: &mut String, road: &Road, look: &Look) {
    // Each with how wide the road is there, where that is not as wide as the
    // game's roads are.
    let wide = |i: usize| {
        road.widths
            .get(i)
            .copied()
            .filter(|&width| width != crate::track::WIDTH)
    };
    let corners = road
        .corners
        .iter()
        .enumerate()
        .map(|(i, corner)| match wide(i) {
            Some(width) => listed(&[corner[0], corner[1], corner[2], corner[3], width]),
            None => listed(corner),
        });
    let about = match road.widths.is_empty() {
        true => "The road's corners, each [x, height, z, radius]. The first is the start.",
        false => {
            "The road's corners, each [x, height, z, radius] and how wide the road is there if not 16. The first is the start."
        }
    };
    section(text, about, "corners", corners.collect());
    section(
        text,
        "Tunnels, each from one place on the road to another.",
        "tunnels",
        road.tunnels
            .iter()
            .map(|tunnel| {
                format!(
                    "{{ from = {}, to = {} }}",
                    listed(&tunnel.from),
                    listed(&tunnel.to)
                )
            })
            .collect(),
    );
    section(
        text,
        "Jumps, each where its lip is.",
        "jumps",
        road.jumps
            .iter()
            .map(|jump| format!("{{ lip = {} }}", listed(&jump.lip)))
            .collect(),
    );
    section(
        text,
        "Ways off the road and back onto it, each from so far round the lap to so far.",
        "byways",
        road.byways
            .iter()
            .map(|byway| {
                format!(
                    "{{ from = {}, to = {}, road = {}, wall = {}, bumps = {}, covered = {}, taken = {} }}",
                    byway.from, byway.to, byway.road, byway.wall, byway.bumps, byway.covered, byway.taken
                )
            })
            .collect(),
    );
    section(
        text,
        "Corners with grass inside them, to be cut across.",
        "infields",
        road.infields
            .iter()
            .map(|infield| format!("{{ from = {}, to = {} }}", infield.from, infield.to))
            .collect(),
    );
    section(
        text,
        "Speed pads, each so far round the lap and so far right of the middle of the road.",
        "pads",
        road.pads
            .iter()
            .map(|pad| match pad.right {
                0.0 => format!("{{ at = {} }}", pad.at),
                right => format!("{{ at = {}, right = {right} }}", pad.at),
            })
            .collect(),
    );
    section(
        text,
        "Banked corners, and how steep each is across the road.",
        "banks",
        road.banks
            .iter()
            .map(|bank| {
                format!(
                    "{{ from = {}, to = {}, tilt = {} }}",
                    bank.from, bank.to, bank.tilt
                )
            })
            .collect(),
    );
    if *look != Look::default() {
        let _ = write!(
            text,
            "\n# What its walls are built of.\nwalls = {{ wall = {}, footing = {}, coping = {}, cap = {}, battlements = {} }}\n",
            colour(look.wall),
            colour(look.footing),
            colour(look.coping),
            colour(look.cap),
            look.battlements
        );
    }
}

/// An arena's floor and what is on it.
fn floor(text: &mut String, plan: &Plan) {
    let pairs = |places: &mut dyn Iterator<Item = &[f32; 2]>| -> Vec<String> {
        places.map(|place| listed(place)).collect()
    };
    section(
        text,
        "The corners of the floor in order, each [x, z], round the middle, which is [0, 0].",
        "outline",
        pairs(&mut plan.outline.iter()),
    );
    let _ = write!(
        text,
        "\n# The floor's two colours.\nfloor = [{}, {}]\n",
        colour(plan.floor[0]),
        colour(plan.floor[1])
    );
    let stood = |at: &[f32; 2], half: &[f32; 2], height: f32| {
        format!(
            "at = {}, half = {}, height = {height}",
            listed(at),
            listed(half)
        )
    };
    section(
        text,
        "What stands on the floor, solid.",
        "blocks",
        plan.blocks
            .iter()
            .map(|block| {
                format!(
                    "{{ {}, colour = {} }}",
                    stood(&block.at, &block.half, block.height),
                    colour(block.colour)
                )
            })
            .collect(),
    );
    section(
        text,
        "Slopes from the floor up, and which way each climbs.",
        "ramps",
        plan.ramps
            .iter()
            .map(|ramp| {
                format!(
                    "{{ {}, climbs = {} }}",
                    stood(&ramp.at, &ramp.half, ramp.height),
                    quoted(compass(ramp.climbs))
                )
            })
            .collect(),
    );
    section(
        text,
        "Floors held up, driven on and under.",
        "decks",
        plan.decks
            .iter()
            .map(|deck| match deck.colour {
                Some(tint) => format!(
                    "{{ {}, colour = {} }}",
                    stood(&deck.at, &deck.half, deck.height),
                    colour(tint)
                ),
                None => format!("{{ {} }}", stood(&deck.at, &deck.half, deck.height)),
            })
            .collect(),
    );
    section(
        text,
        "The loop the computer's cars go round, each place [x, height, z].",
        "line",
        plan.line.iter().map(|place| listed(place)).collect(),
    );
    section(
        text,
        "Where the six cars start.",
        "grid",
        plan.grid
            .iter()
            .map(|start| match start.faces {
                Some(faces) => format!(
                    "{{ at = {}, faces = {} }}",
                    listed(&start.at),
                    quoted(compass(faces))
                ),
                None => listed(&start.at),
            })
            .collect(),
    );
    section(
        text,
        "The bricks, each where it lies.",
        "bricks",
        plan.bricks
            .iter()
            .map(|brick| {
                let hue = match brick.hue {
                    Hue::Any => None,
                    Hue::White => Some("white"),
                    Hue::Is(Power::Red) => Some("red"),
                    Hue::Is(Power::Yellow) => Some("yellow"),
                    Hue::Is(Power::Blue) => Some("blue"),
                    Hue::Is(Power::Green) => Some("green"),
                    Hue::Changing => Some("changing"),
                };
                match (hue, brick.low) {
                    (None, false) => listed(&brick.at),
                    (hue, low) => {
                        let mut line = format!("{{ at = {}", listed(&brick.at));
                        if let Some(hue) = hue {
                            let _ = write!(line, ", colour = {}", quoted(hue));
                        }
                        if low {
                            line += ", low = true";
                        }
                        line + " }"
                    }
                }
            })
            .collect(),
    );
    if let Some(rows) = plan.rows {
        let _ = write!(
            text,
            "\n# How many rows of bricks lie across the line besides.\nrows = {rows}\n"
        );
    }
    section(
        text,
        "Speed pads on the floor, and the way each points, in degrees round from east to north.",
        "pads",
        plan.pads
            .iter()
            .map(|pad| {
                format!(
                    "{{ place = {}, heading = {} }}",
                    listed(&pad.at),
                    pad.heading
                )
            })
            .collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_written_out_reads_back_as_itself() {
        for map in crate::map::stock() {
            let text = map.written();
            let read = Map::tried(&map.key, &text)
                .unwrap_or_else(|wrong| panic!("{}: {wrong}\n{text}", map.key));
            assert_eq!(
                Map {
                    text: map.text.clone(),
                    ..read
                },
                *map,
                "{}",
                map.key
            );
            // And what is written of that is the same again.
            assert_eq!(Map::read(&map.key, &text).unwrap().written(), text);
        }
    }

    #[test]
    fn how_wide_a_road_is_at_its_corners_is_written_where_it_is_not_as_wide_as_the_game_s() {
        let mut map = crate::map::named("BRICK").clone();
        map.road.widths = vec![crate::track::WIDTH; map.road.corners.len()];
        (map.road.widths[1], map.road.widths[2]) = (24.0, 11.5);
        let text = map.written();
        assert!(
            text.contains("[88, 0, 8, 32, 24],")
                && text.contains("[88, 0, 92, 20, 11.5],")
                && text.contains("[-52, 0, 8, 0],")
        );
        let read = Map::read("BRICK", &text).unwrap();
        assert_eq!(read.road.widths, map.road.widths);
        // As wide as the game's all the way round, it says nothing of it.
        map.road.widths = vec![crate::track::WIDTH; map.road.corners.len()];
        assert_eq!(
            Map::read("BRICK", &map.written()).unwrap().road.widths,
            Vec::<f32>::new()
        );
    }

    #[test]
    fn what_a_file_can_t_have_bare_is_written_so_that_it_can() {
        let mut map = crate::map::named("BRICK").clone();
        map.name = "The \"Brick\" \\ Circuit".into();
        map.look.wall = Color::srgb(0.25, 0.5, 0.125);
        let read = Map::read("BRICK", &map.written()).unwrap();
        assert_eq!((read.name, read.look), (map.name, map.look));
        assert_eq!(compass([0.0, -1.0]), "north");
        assert_eq!(colour(crate::meshgen::DARK_GREY), "\"dark grey\"");
    }
}
