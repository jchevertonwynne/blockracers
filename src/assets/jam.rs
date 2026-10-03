//! `LEGO.JAM`: a flat archive with a directory tree. Each directory is a list of
//! (name, offset, size) files followed by a list of (name, offset) sub-directories.

use std::collections::HashMap;
use std::path::Path;

pub struct Jam {
    data: Vec<u8>,
    /// Upper-case path (`/GAMEDATA/RACEC0R0/RKTK.GDB`) to (offset, size).
    files: HashMap<String, (usize, usize)>,
}

fn u32_at(d: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn name_at(d: &[u8], at: usize) -> Option<String> {
    let raw = d.get(at..at + 12)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(12);
    Some(String::from_utf8_lossy(&raw[..end]).to_uppercase())
}

impl Jam {
    pub fn open(path: impl AsRef<Path>) -> Option<Jam> {
        let data = std::fs::read(path).ok()?;
        if data.get(..4)? != b"LJAM" {
            return None;
        }
        let mut files = HashMap::new();
        let mut dirs = vec![(4usize, String::new())];
        while let Some((mut at, prefix)) = dirs.pop() {
            let count = u32_at(&data, at)?;
            at += 4;
            for _ in 0..count {
                let entry = (u32_at(&data, at + 12)?, u32_at(&data, at + 16)?);
                files.insert(format!("{prefix}/{}", name_at(&data, at)?), entry);
                at += 20;
            }
            let count = u32_at(&data, at)?;
            at += 4;
            for _ in 0..count {
                dirs.push((u32_at(&data, at + 12)?, format!("{prefix}/{}", name_at(&data, at)?)));
                at += 16;
            }
        }
        Some(Jam { data, files })
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        let &(offset, size) = self.files.get(&path.to_uppercase())?;
        self.data.get(offset..offset + size)
    }

    /// Paths of every file directly inside `dir`.
    pub fn list<'a>(&'a self, dir: &str) -> impl Iterator<Item = &'a str> {
        let prefix = format!("{}/", dir.to_uppercase());
        self.files
            .keys()
            .filter(move |p| p.strip_prefix(prefix.as_str()).is_some_and(|rest| !rest.contains('/')))
            .map(String::as_str)
    }
}
