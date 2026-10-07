//! Who races where. The game's tables give each race its circuit (`LEGORACE.RCB`),
//! each circuit its drivers (`LEGORACE.CRB`), each driver a figure and a champion's
//! car (`DRIVERS.DDB`), and each champion the models of that car (`CHAMPS.CCB`).

use crate::assets::lrs::Cosmetics;
use crate::assets::{
    Jam,
    tokens::{Token, tokenize},
};

/// One racer of a race's field.
#[derive(Clone, Debug, PartialEq)]
pub struct Driver {
    /// The game's short name for the driver, which its figure's files go by.
    pub code: String,
    pub name: &'static str,
    /// The model of the driver's figure.
    pub figure: String,
    /// What the files of the car's body go by, less the `cm` they end in.
    pub car: String,
    pub chassis: String,
    /// What the champion's car weighs, and where its centre of mass is from the
    /// car's origin, in the game's units and axes (`ChampionDefinitionList`).
    pub mass: f32,
    pub centre: [f32; 3],
    /// Which set of voice clips is the driver's.
    pub voice: usize,
    /// How keen the driver is on each colour of brick: red, yellow, green, blue.
    pub keenness: [i32; 4],
    /// The colour the driver saves white bricks up for (1 red, 2 blue, 3 green,
    /// 4 yellow; 0 for none), and how many.
    pub charge: (i32, i32),
}

impl Driver {
    /// What the driver's voice clips may go by: the figure's name, or the car's.
    pub fn voices(&self) -> [String; 2] {
        [
            self.figure.trim_end_matches("PELVIS").to_string(),
            self.car.clone(),
        ]
    }
}

/// A race as the game's tables list it.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceEntry {
    pub name: String,
    /// The folder its data is in.
    pub folder: String,
    pub circuit: String,
    /// Where in its circuit it comes.
    pub round: usize,
    pub mirrored: bool,
}

/// The drivers' names. The game shows faces, not names, so these are not in its data.
pub const NAMES: [(&str, &str); 24] = [
    ("RR", "Rocket Racer"),
    ("VV", "Veronica Voltage"),
    ("CR", "Captain Redbeard"),
    ("KK", "King Kahuka"),
    ("BB", "Basil the Batlord"),
    ("JT", "Johnny Thunder"),
    ("BVB", "Baron von Barron"),
    ("GM", "Gypsy Moth"),
    ("GB", "Governor Broadside"),
    ("RH", "Robin Hood"),
    ("AD", "Ann Droid"),
    ("PH", "Pharaoh Hotep"),
    ("IL", "Islander"),
    ("RK", "Royal King"),
    ("CC", "Commander Cold"),
    ("TC", "Achu"),
    ("WW", "Willa the Witch"),
    ("BH", "Black Jack Hawkins"),
    ("SB", "Sam Sinister"),
    ("AP", "Alpha Draconis"),
    ("BK", "Black Knight"),
    ("AL", "Rigel"),
    ("GS", "Gail Storm"),
    ("NH", "Nova Hunter"),
];

/// The player's stand-in in the tables, Veronica Voltage: who they race as until
/// they have built a racer, and whose voice a built racer has.
pub const PLAYER: &str = "VV";

/// The entries of a table, `key "name" { fields }` each, as the name and its fields.
fn entries(tokens: &[Token]) -> Vec<(String, &[Token])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 < tokens.len() {
        if let (Token::Key(0x27), Token::Str(name), Token::LCurly) =
            (&tokens[at], &tokens[at + 1], &tokens[at + 2])
        {
            // Fields may hold a braced list of their own.
            let mut depth = 0;
            let end = tokens[at + 2..].iter().position(|t| {
                depth += (*t == Token::LCurly) as i32 - (*t == Token::RCurly) as i32;
                depth == 0
            });
            let end = at + 2 + end.unwrap_or(tokens.len() - at - 3);
            out.push((name.clone(), &tokens[at + 3..end]));
            at = end;
        }
        at += 1;
    }
    out
}

fn text(fields: &[Token], key: u16) -> Option<String> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1)? {
        Token::Str(value) => Some(value.clone()),
        _ => None,
    }
}

/// The `count` numbers a key is followed by.
fn floats(fields: &[Token], key: u16, count: usize) -> Option<Vec<f32>> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    fields
        .get(at + 1..at + 1 + count)?
        .iter()
        .map(|t| match t {
            Token::Float(v) => Some(*v),
            Token::Int(v) => Some(*v as f32),
            _ => None,
        })
        .collect()
}

fn number(fields: &[Token], key: u16) -> Option<i32> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1)? {
        Token::Int(value) => Some(*value),
        Token::Float(value) => Some(*value as i32),
        _ => None,
    }
}

/// Every race in the game's tables, in their order.
pub fn races(jam: &Jam) -> Vec<RaceEntry> {
    let tokens = tokenize(jam.get("/MENUDATA/LEGORACE.RCB").unwrap_or_default());
    entries(&tokens)
        .into_iter()
        .filter_map(|(name, fields)| {
            Some(RaceEntry {
                name,
                folder: text(fields, 0x29)?.to_uppercase(),
                circuit: text(fields, 0x2a)?,
                round: number(fields, 0x28)? as usize,
                mirrored: fields.contains(&Token::Key(0x2c)),
            })
        })
        .collect()
}

/// The circuits in order, each with its races in the order they are run.
pub fn circuits(jam: &Jam) -> Vec<(String, Vec<RaceEntry>)> {
    let tokens = tokenize(jam.get("/MENUDATA/LEGORACE.CRB").unwrap_or_default());
    let races = races(jam);
    entries(&tokens)
        .into_iter()
        .map(|(circuit, _)| {
            let mut rounds: Vec<RaceEntry> = races
                .iter()
                .filter(|r| r.circuit == circuit)
                .cloned()
                .collect();
            rounds.sort_by_key(|r| r.round);
            (circuit, rounds)
        })
        .collect()
}

/// The part set a circuit's winner is given, counted from the first that is won
/// (`CircuitDefinition::GetStringIndex`); a circuit may have none.
pub fn part_set(jam: &Jam, circuit: &str) -> Option<usize> {
    let tokens = tokenize(jam.get("/MENUDATA/LEGORACE.CRB")?);
    let (_, fields) = entries(&tokens)
        .into_iter()
        .find(|(name, _)| name == circuit)?;
    usize::try_from(number(fields, 0x2a)?).ok()
}

/// What the minifigure of one of the game's drivers is made of, by the number the
/// menus' table of them gives the driver (`DriverCosmeticTable::CopyCosmetics`).
pub fn cosmetics(jam: &Jam, id: usize) -> Option<Cosmetics> {
    parts(jam, |_, fields| number(fields, 0x33) == Some(id as i32))
}

/// The same, by the game's short name for the driver.
pub fn cosmetics_of(jam: &Jam, code: &str) -> Option<Cosmetics> {
    parts(jam, |name, _| name.eq_ignore_ascii_case(code))
}

fn parts(jam: &Jam, which: impl Fn(&str, &[Token]) -> bool) -> Option<Cosmetics> {
    let drivers = tokenize(jam.get("/MENUDATA/PARTDB/DRIVERS.DDB")?);
    let (_, fields) = entries(&drivers)
        .into_iter()
        .find(|(name, fields)| which(name, fields))?;
    let part = |key: u16| number(fields, key).map(|at| at as u8);
    Some(Cosmetics {
        hat: part(0x35)?,
        face: part(0x36)?,
        torso: part(0x37)?,
        legs: part(0x38)?,
        expression: 0,
    })
}

/// One driver, by the game's short name for them.
pub fn driver(jam: &Jam, code: &str) -> Option<Driver> {
    let drivers = tokenize(jam.get("/GAMEDATA/COMMON/DRIVERS.DDB")?);
    let (_, fields) = entries(&drivers)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(code))?;
    let champion = text(fields, 0x2b)?;
    let champions = tokenize(jam.get("/GAMEDATA/COMMON/CHAMPS.CCB")?);
    let (_, car) = entries(&champions)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&champion))?;
    let code = code.to_uppercase();
    Some(Driver {
        name: NAMES.iter().find(|n| n.0 == code).map_or("Racer", |n| n.1),
        code,
        figure: text(fields, 0x2a)?.to_uppercase(),
        car: text(car, 0x29)?.trim_end_matches("cm").to_uppercase(),
        chassis: text(car, 0x2b)?,
        mass: floats(car, 0x2c, 1).map_or(0.0, |v| v[0]),
        centre: floats(car, 0x2d, 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]),
        voice: number(fields, 0x34).unwrap_or(1) as usize,
        keenness: [0x2c, 0x2d, 0x2e, 0x2f].map(|key| number(fields, key).unwrap_or(0)),
        charge: {
            let at = fields.iter().position(|t| *t == Token::Key(0x3a));
            let int = |i: usize| match at.and_then(|at| fields.get(at + i)) {
                Some(Token::Int(v)) => *v,
                _ => 0,
            };
            (int(1), int(2))
        },
    })
}

/// The field for a race in one of the game's folders: the circuit's drivers, with the
/// player in place of the first of them and last in the list.
pub fn field(jam: &Jam, circuit: &str) -> Vec<Driver> {
    let tokens = tokenize(jam.get("/MENUDATA/LEGORACE.CRB").unwrap_or_default());
    let codes: Vec<String> = entries(&tokens)
        .into_iter()
        .find(|(name, _)| name == circuit)
        .map(|(_, fields)| {
            fields
                .iter()
                .filter_map(|t| {
                    if let Token::Str(code) = t {
                        Some(code.clone())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    // The list's first names are the driver the player replaces and the next circuit.
    let codes = codes
        .iter()
        .filter(|c| c.chars().all(|ch| ch.is_ascii_uppercase()))
        .skip(1);
    codes
        .chain([&PLAYER.to_string()])
        .filter_map(|code| driver(jam, code))
        .collect()
}

/// The circuit a folder's race is run in when raced on its own.
pub fn circuit_of(jam: &Jam, folder: &str) -> Option<String> {
    races(jam)
        .into_iter()
        .find(|r| r.folder.eq_ignore_ascii_case(folder) && !r.mirrored)
        .map(|r| r.circuit)
}

#[cfg(test)]
#[test]
fn the_first_circuit_is_captain_redbeard_s() {
    let Some(jam) = Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
        return;
    };
    let circuits = circuits(&jam);
    assert_eq!(circuits.len(), 7);
    let rounds = |n: usize| {
        circuits[n]
            .1
            .iter()
            .map(|r| (r.folder.as_str(), r.mirrored))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rounds(0),
        [
            ("RACEC0R1", false),
            ("RACEC1R0", false),
            ("RACEC0R3", false),
            ("RACEC0R2", false)
        ]
    );
    assert_eq!(
        rounds(3),
        [
            ("RACEC0R2", true),
            ("RACEC0R3", true),
            ("RACEC1R0", true),
            ("RACEC0R1", true)
        ]
    );
    assert_eq!(rounds(6), [("RACEC3R0", false)]);
    assert_eq!(circuit_of(&jam, "RACEC0R0").as_deref(), Some("c1"));
    let field = field(&jam, "c0");
    assert_eq!(
        field.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["CR", "GB", "RH", "AD", "PH", "VV"]
    );
    assert_eq!(
        (
            field[0].name,
            field[0].car.as_str(),
            field[0].chassis.as_str()
        ),
        ("Captain Redbeard", "CR", "crchas0")
    );
    // Lesser drivers share a car; some cars go by another name than their driver's.
    assert_eq!(
        (field[3].car.as_str(), field[3].chassis.as_str()),
        ("BK", "bkchas0")
    );
    assert_eq!(driver(&jam, "SB").unwrap().car, "SS");
    assert_eq!(driver(&jam, "GM").unwrap().chassis, "gm_chas0");
    for (circuit, _) in &circuits {
        assert_eq!(self::field(&jam, circuit).len(), 6, "{circuit}");
    }
}

#[cfg(test)]
#[test]
fn the_menus_know_what_each_champion_is_made_of() {
    let Some(jam) = crate::world::jam() else {
        return;
    };
    // Captain Redbeard, whom the main menu shows to begin with, and Rocket Racer.
    let redbeard = cosmetics(&jam, 0x13).unwrap();
    assert_eq!(
        (redbeard.hat, redbeard.face, redbeard.torso, redbeard.legs),
        (4, 4, 24, 10)
    );
    assert_eq!(cosmetics(&jam, 0).unwrap().hat, 1);
    assert!(cosmetics(&jam, 99).is_none());
    assert_eq!(cosmetics_of(&jam, "cr"), Some(redbeard));
    // Everyone a player can race as online has a figure to celebrate with.
    for (code, _) in NAMES {
        assert!(cosmetics_of(&jam, code).is_some(), "{code}");
    }
}
