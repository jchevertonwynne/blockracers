//! The display settings, none of which the original has in this form: whether frames
//! wait for the display, whether the game fills the screen, and whether edges are
//! smoothed.

use crate::menu::Settings;
use bevy::{
    prelude::*,
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};

pub fn apply(
    mut commands: Commands,
    settings: Res<Settings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    camera: Single<(Entity, Option<&Msaa>), With<Camera3d>>,
) {
    if !settings.is_changed() {
        return;
    }
    let present = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    if window.present_mode != present {
        window.present_mode = present;
    }
    let mode = if settings.fullscreen { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
    if window.mode != mode {
        window.mode = mode;
    }
    let msaa = if settings.smoothing { Msaa::Sample4 } else { Msaa::Off };
    let (entity, current) = *camera;
    if current != Some(&msaa) {
        commands.entity(entity).insert(msaa);
    }
}
