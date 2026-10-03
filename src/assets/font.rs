//! The original's bitmap fonts (`.FDB` and a picture per font) and string tables
//! (`.SRF`). A font's picture is one row of glyphs, set apart by columns that repeat
//! its first column; the `.FDB` says which character each glyph is. Follows
//! `GolFontLibrary`, `GolFontBase` and `GolStringTable`.

use super::image::{Pixels, decode_bmp};
use super::tokens::{Token, tokenize};
use super::Jam;
use std::collections::HashMap;

pub struct Font {
    pixels: Pixels,
    /// Where each character's glyph starts in the picture, and how wide it is.
    glyphs: HashMap<char, (u32, u32)>,
    space: u32,
    /// Extra room between characters; may be negative.
    spacing: i32,
}

impl Font {
    fn column(&self, x: u32) -> impl Iterator<Item = &[u8]> {
        let width = self.pixels.width as usize;
        (0..self.pixels.height as usize).map(move |y| {
            let at = (y * width + x as usize) * 4;
            &self.pixels.rgba[at..at + 3]
        })
    }

    fn separator(&self, x: u32) -> bool {
        x >= self.pixels.width || self.column(x).eq(self.column(0))
    }

    /// `GolFontBase::ScanGlyphs`: the run of separator columns at the start is as
    /// wide as a space, and the glyphs follow, each up to the next separator.
    fn scan(&mut self, characters: &[char]) {
        let width = self.pixels.width;
        self.space = (0..width).find(|&x| !self.separator(x)).unwrap_or(width);
        let mut x = self.space + 1;
        for (i, &character) in characters.iter().enumerate() {
            if i > 0 {
                while x < width && self.separator(x) {
                    x += 1;
                }
            }
            let start = x;
            while x < width && !self.separator(x) {
                x += 1;
            }
            self.glyphs.insert(character, (start, x - start));
        }
    }

    /// The picture the glyphs are cut from.
    pub fn pixels(&self) -> &Pixels {
        &self.pixels
    }

    /// Where a character's glyph is in the picture, and its width.
    pub fn glyph(&self, character: char) -> Option<(u32, u32)> {
        self.glyphs.get(&character).copied()
    }

    /// How far to move on after a character.
    pub fn advance(&self, character: char) -> i32 {
        self.glyphs.get(&character).map_or(self.space, |g| g.1) as i32 + self.spacing
    }

    /// The width of a line of text.
    pub fn measure(&self, line: &str) -> f32 {
        self.width(&line.to_uppercase()) as f32
    }

    pub fn height(&self) -> u32 {
        self.pixels.height
    }

    fn width(&self, line: &str) -> u32 {
        let advance = |c: char| self.glyphs.get(&c).map_or(self.space, |g| g.1) as i32 + self.spacing;
        (line.chars().map(advance).sum::<i32>() - self.spacing).max(0) as u32
    }

    /// Draws `text`, one line under another, each centred if asked.
    pub fn render(&self, text: &str, centred: bool) -> Pixels {
        let text = text.to_uppercase();
        let lines: Vec<&str> = text.split('\n').collect();
        let width = lines.iter().map(|l| self.width(l)).max().unwrap_or(0).max(1);
        let height = self.height() * lines.len() as u32;
        let mut out = Pixels { width, height, rgba: vec![0; (width * height * 4) as usize] };
        for (row, line) in lines.iter().enumerate() {
            let mut x = if centred { (width - self.width(line)) as i32 / 2 } else { 0 };
            for character in line.chars() {
                let Some(&(from, glyph_width)) = self.glyphs.get(&character) else {
                    x += self.space as i32 + self.spacing;
                    continue;
                };
                for gx in 0..glyph_width {
                    let to_x = x + gx as i32;
                    if to_x < 0 || to_x >= width as i32 {
                        continue;
                    }
                    for y in 0..self.height() {
                        let source = ((y * self.pixels.width + from + gx) * 4) as usize;
                        let target = (((row as u32 * self.height() + y) * width + to_x as u32) * 4) as usize;
                        if self.pixels.rgba[source + 3] > 0 {
                            out.rgba[target..target + 4].copy_from_slice(&self.pixels.rgba[source..source + 4]);
                        }
                    }
                }
                x += glyph_width as i32 + self.spacing;
            }
        }
        out
    }
}

/// Loads the fonts an `.FDB` in `dir` lists, by name.
pub fn load_fonts(jam: &Jam, dir: &str, file: &str) -> HashMap<String, Font> {
    let tokens = tokenize(jam.get(&format!("{dir}/{file}")).unwrap_or_default());
    let mut fonts = HashMap::new();
    for (i, token) in tokens.iter().enumerate().skip(1) {
        let (Token::Key(0x27), Some(Token::Str(name)), Some(Token::LCurly)) = (token, tokens.get(i + 1), tokens.get(i + 2))
        else {
            continue;
        };
        let number = |at: usize| match tokens.get(at) {
            Some(Token::Int(v)) => *v,
            Some(Token::Float(v)) => *v as i32,
            _ => 0,
        };
        let (mut key, mut spacing, mut characters) = (None, 0, Vec::new());
        let mut at = i + 3;
        while let Some(token) = tokens.get(at) {
            match token {
                Token::RCurly => break,
                Token::Key(0x2a) => key = Some([number(at + 1) as u8, number(at + 2) as u8, number(at + 3) as u8]),
                Token::Key(0x2c) => spacing = number(at + 1),
                Token::Key(0x2b) => {
                    // `[ "A" "B" 5 ... ]`: characters, or the codes of special ones.
                    at += 2;
                    while let Some(token) = tokens.get(at) {
                        match token {
                            Token::Str(s) => characters.extend(s.chars().next()),
                            Token::Int(code) => characters.extend(char::from_u32(*code as u32)),
                            _ => break,
                        }
                        at += 1;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        let Some(pixels) = jam.get(&format!("{dir}/{name}.BMP")).and_then(|d| decode_bmp(d, key)) else { continue };
        let mut font = Font { pixels, glyphs: HashMap::new(), space: 0, spacing };
        font.scan(&characters);
        fonts.insert(name.to_lowercase(), font);
    }
    fonts
}

/// The strings of an `.SRF` table: a count, a length, offsets, then 16-bit characters.
pub fn load_strings(data: &[u8]) -> Vec<String> {
    let word = |at: usize| data.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let (count, length) = (word(0), word(2));
    let text = 4 + count * 2;
    (0..count)
        .map(|i| {
            let from = word(4 + i * 2);
            let characters = (from..length).map(|at| word(text + at * 2) as u32).take_while(|&c| c != 0);
            characters.filter_map(char::from_u32).collect()
        })
        .collect()
}

#[cfg(test)]
#[test]
fn menu_fonts_and_strings_load() {
    let Some(jam) = Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let fonts = load_fonts(&jam, "/MENUDATA/ENGLISH", "GFONTS.FDB");
    assert_eq!(fonts.len(), 4);
    for (name, font) in &fonts {
        let widths: Vec<u32> = "AIW".chars().map(|c| font.glyphs[&c].1).collect();
        println!("{name}: {}x{}, space {}, {} glyphs, A I W = {widths:?}", font.pixels.width, font.height(), font.space, font.glyphs.len());
        assert!(font.glyphs.values().all(|g| g.1 > 0 && g.0 + g.1 <= font.pixels.width), "{name}");
        assert!(widths[1] < widths[2], "{name}");
    }
    let drawn = fonts["font_ths"].render("Single\nRace", true);
    assert_eq!(drawn.height, fonts["font_ths"].height() * 2);
    assert!(drawn.rgba.chunks(4).any(|p| p[3] > 0));

    let strings = load_strings(jam.get("/MENUDATA/ENGLISH/MENUTEXT.SRF").unwrap());
    assert_eq!((strings.len(), strings[34].as_str(), strings[39].as_str()), (192, "SINGLE RACE", "QUIT"));
    let circuits = load_strings(jam.get("/MENUDATA/ENGLISH/CIRCUIT.SRF").unwrap());
    assert_eq!(circuits[4], "ROYAL KNIGHTS RACEWAY");
    for image in ["BACKDRP", "RACERS", "ARROWLU", "TUL", "TT", "BAR", "TAB", "PIRATE", "CLEAR32", "TXTAROL"] {
        let pixels = decode_bmp(jam.get(&format!("/MENUDATA/{image}.BMP")).unwrap(), None).unwrap();
        println!("{image}: {}x{}", pixels.width, pixels.height);
    }
}
