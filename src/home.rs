//! Where the game keeps what it keeps from one session to the next: one folder,
//! `~/.brickracers` (`$BRICK_HOME` says another), with the settings, the garage,
//! what has been won, the ghosts and the maps in it. The port's own.
//!
//! Each thing kept there has a variable that says somewhere else for it alone
//! (`BRICK_SETTINGS` and the rest), which is what a demo and a test use.

use std::path::{Path, PathBuf};

/// What the folder is called, in the player's home folder.
const FOLDER: &str = ".brickracers";
/// What was kept loose in the home folder before there was one folder for it all,
/// and what each is called in that folder.
const LOOSE: [(&str, &str); 4] = [
    (".brick_racers_settings", "settings"),
    (".brick_racers_garage", "garage"),
    (".brick_racers_progress", "progress"),
    (".brick_racers_ghosts", "ghosts"),
];

/// The folder: `$BRICK_HOME`, or the one in the home folder.
fn folder() -> Option<PathBuf> {
    match std::env::var_os("BRICK_HOME") {
        Some(folder) => Some(folder.into()),
        None => Some(PathBuf::from(std::env::var_os("HOME")?).join(FOLDER)),
    }
}

/// Where one of the things kept is: wherever `variable` says, or under `name` in
/// the folder.
pub fn kept(variable: &str, name: &str) -> Option<PathBuf> {
    match std::env::var_os(variable) {
        Some(file) => Some(file.into()),
        None => Some(folder()?.join(name)),
    }
}

/// Says something of what is kept as the game starts, which is before there is a
/// log to say it in.
pub fn say(what: &str) {
    eprintln!("Brick Racers: {what}");
}

/// Writes a file, making the folder it is in if that isn't there yet.
pub fn write(file: &Path, data: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(file, data)
}

/// Moves what is loose in the home folder into the game's own folder there, where
/// that has nothing of the same name yet.
pub fn gather() {
    // Only into the folder that is in the home folder itself.
    if std::env::var_os("BRICK_HOME").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        gather_in(Path::new(&home));
    }
}

fn gather_in(home: &Path) {
    for (loose, name) in LOOSE {
        let (from, to) = (home.join(loose), home.join(FOLDER).join(name));
        if !from.exists() || to.exists() {
            continue;
        }
        let moved =
            std::fs::create_dir_all(home.join(FOLDER)).and_then(|_| std::fs::rename(&from, &to));
        match moved {
            Ok(()) => say(&format!("moved {} to {}", from.display(), to.display())),
            Err(error) => say(&format!(
                "could not move {} to {}: {error}",
                from.display(),
                to.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_loose_in_the_home_folder_is_gathered_once() {
        let home = std::env::temp_dir().join(format!("brick_home_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".brick_racers_ghosts")).unwrap();
        std::fs::write(home.join(".brick_racers_ghosts/RACEC0R0"), "a run").unwrap();
        std::fs::write(home.join(".brick_racers_settings"), "laps=2\n").unwrap();
        std::fs::write(home.join(".brick_racers_garage"), [1, 2, 3]).unwrap();
        gather_in(&home);
        let read = |name: &str| std::fs::read_to_string(home.join(name)).ok();
        assert_eq!(read(".brickracers/settings").as_deref(), Some("laps=2\n"));
        assert_eq!(
            read(".brickracers/ghosts/RACEC0R0").as_deref(),
            Some("a run")
        );
        assert_eq!(
            std::fs::read(home.join(".brickracers/garage")).unwrap(),
            [1, 2, 3]
        );
        assert!(!home.join(".brick_racers_settings").exists());
        assert!(!home.join(".brick_racers_ghosts").exists());
        // Nothing had been won, and nothing is made up; and what is in the folder
        // already is not written over by something loose that turns up later.
        assert!(!home.join(".brickracers/progress").exists());
        std::fs::write(home.join(".brick_racers_settings"), "laps=4\n").unwrap();
        gather_in(&home);
        assert_eq!(read(".brickracers/settings").as_deref(), Some("laps=2\n"));
        assert_eq!(read(".brick_racers_settings").as_deref(), Some("laps=4\n"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
