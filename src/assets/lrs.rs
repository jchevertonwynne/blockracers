//! `.LRS` files: saved racers. The game's own (`QBUILD.LRS`, the cars its quick build
//! hands out, and `DEFAULT.LRS`) and the file the port keeps the player's in are the
//! same thing: a header, then each racer's record in blocks of 127 bytes, every block
//! followed by the sum of its bytes. After `SaveGame` and `SaveRecordList::Record`.

use serde::{Deserialize, Serialize};

const MAGIC: [u8; 2] = *b"LR";
/// Where the records begin; what comes before is the game's settings and best times.
const HEADER: usize = 0x480;
const BLOCK: usize = 127;
const RECORD: usize = 0x22d;
const NAME: usize = 0x00;
/// The longest a racer's name may be.
pub const NAME_LENGTH: usize = 14;
const COSMETICS: usize = 0x1c;
/// Set in the expression's byte once the car is as it was last saved or handed out.
const CAR_SAVED: u8 = 0x80;
const CHASSIS: usize = 0x21;
const CAR: usize = 0x29;
const CAR_LENGTH: usize = 0x202;

/// What a racer's minifigure is made of, each a place in the part catalogue's lists.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Cosmetics {
    pub hat: u8,
    pub face: u8,
    pub torso: u8,
    pub legs: u8,
    pub expression: u8,
}

/// A racer someone has built: a name, a minifigure and a car.
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Racer {
    pub name: String,
    pub cosmetics: Cosmetics,
    /// The chassis table's name for the car's chassis.
    pub chassis: String,
    /// The car's pieces, as `build::Car` writes them.
    pub car: Vec<u8>,
    /// Whether the car is one the game handed out and nothing has been done to since.
    pub stock: bool,
}

impl Racer {
    fn from_record(data: &[u8]) -> Racer {
        let name = data[NAME..NAME + NAME_LENGTH * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .filter_map(|c| char::from_u32(c as u32))
            .collect();
        let chassis = &data[CHASSIS..CHASSIS + 8];
        let chassis = &chassis[..chassis.iter().position(|&b| b == 0).unwrap_or(8)];
        // The pieces are counted in the first two bytes, eight bytes each.
        let pieces = u16::from_be_bytes([data[CAR], data[CAR + 1]]) as usize;
        let length = (2 + pieces * 8).min(CAR_LENGTH);
        Racer {
            name,
            cosmetics: Cosmetics {
                face: data[COSMETICS],
                hat: data[COSMETICS + 1],
                legs: data[COSMETICS + 2],
                torso: data[COSMETICS + 3],
                expression: data[COSMETICS + 4] & !CAR_SAVED,
            },
            chassis: String::from_utf8_lossy(chassis).to_lowercase(),
            car: data[CAR..CAR + length].to_vec(),
            stock: data[COSMETICS + 4] & CAR_SAVED != 0,
        }
    }

    fn record(&self) -> [u8; RECORD] {
        let mut data = [0; RECORD];
        for (at, c) in self.name.encode_utf16().take(NAME_LENGTH).enumerate() {
            data[NAME + at * 2..NAME + at * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
        let c = self.cosmetics;
        let saved = if self.stock { CAR_SAVED } else { 0 };
        data[COSMETICS..COSMETICS + 5].copy_from_slice(&[
            c.face,
            c.hat,
            c.legs,
            c.torso,
            c.expression | saved,
        ]);
        let chassis = self.chassis.as_bytes();
        let length = chassis.len().min(8);
        data[CHASSIS..CHASSIS + length].copy_from_slice(&chassis[..length]);
        let length = self.car.len().min(CAR_LENGTH);
        data[CAR..CAR + length].copy_from_slice(&self.car[..length]);
        data
    }
}

fn checksum(block: &[u8]) -> u8 {
    block.iter().fold(0u8, |sum, &b| sum.wrapping_add(b))
}

/// Every racer of a file; none of one that isn't a save or has been damaged.
pub fn read(file: &[u8]) -> Vec<Racer> {
    let mut racers = Vec::new();
    if file.len() < 5 || file[..2] != MAGIC {
        return racers;
    }
    let mut at = u16::from_le_bytes([file[3], file[4]]) as usize;
    for _ in 0..file[2] {
        let mut data = Vec::with_capacity(RECORD + BLOCK);
        while data.len() < RECORD {
            let Some(block) = file.get(at..at + BLOCK + 1) else {
                return racers;
            };
            if checksum(&block[..BLOCK]) != block[BLOCK] {
                return racers;
            }
            data.extend_from_slice(&block[..BLOCK]);
            at += BLOCK + 1;
        }
        racers.push(Racer::from_record(&data));
    }
    racers
}

/// A file of these racers, as the game would write it; its settings are left blank.
pub fn write(racers: &[Racer]) -> Vec<u8> {
    let mut file = vec![0; HEADER];
    file[..2].copy_from_slice(&MAGIC);
    file[2] = racers.len().min(255) as u8;
    file[3..5].copy_from_slice(&(HEADER as u16).to_le_bytes());
    for racer in racers.iter().take(255) {
        for chunk in racer.record().chunks(BLOCK) {
            let mut block = [0; BLOCK];
            block[..chunk.len()].copy_from_slice(chunk);
            file.extend_from_slice(&block);
            file.push(checksum(&block));
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Jam;

    #[test]
    fn the_quick_build_cars_are_read_and_written_back_the_same() {
        let Some(jam) = Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
            return;
        };
        let racers = read(jam.get("/MENUDATA/QBUILD.LRS").unwrap());
        assert_eq!(racers.len(), 24);
        assert_eq!(racers[0].chassis, "crchas0");
        assert_eq!(racers[3].chassis, "rrchas0");
        // Each is a chassis and some bricks on it.
        assert!(racers.iter().all(|r| r.car.len() > 10));
        assert_eq!(racers[1].car[..10], [0, 8, 0, 11, 0, 0, 0, 0, 0, 0]);
        assert_eq!(read(&write(&racers)), racers);
        assert_eq!(read(jam.get("/MENUDATA/DEFAULT.LRS").unwrap()).len(), 4);
    }

    #[test]
    fn a_racer_keeps_its_name_and_its_figure() {
        let racer = Racer {
            name: "Brickbeard".into(),
            cosmetics: Cosmetics {
                hat: 4,
                face: 5,
                torso: 25,
                legs: 3,
                expression: 2,
            },
            chassis: "gm_chas0".into(),
            car: vec![0, 1, 0, 12, 0, 0, 0, 3, 0, 0],
            stock: false,
        };
        assert_eq!(read(&write(std::slice::from_ref(&racer))), [racer]);
        assert!(read(b"not a save").is_empty());
    }
}
