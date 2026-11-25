//! The launch check CI runs on every build: with `LIBROXEL_SMOKE_SECONDS`
//! set to a number of seconds, the app opens as usual, draws frames for
//! that long, prints how many it drew and quits. It exits with 0 when it
//! drew frames and 1 when it drew none, so a build that links but can't
//! open a window or draw shows up before a release.
//!
//! A smoke run leaves the computer alone: no update check, no usage
//! pings, no `libroxel` command or link handler set up.

use bevy::prelude::*;
use std::time::{Duration, Instant};

/// How long a smoke run lasts, when this is one.
pub fn seconds() -> Option<Duration> {
    let text = std::env::var("LIBROXEL_SMOKE_SECONDS").ok()?;
    text.trim().parse::<f64>().ok().filter(|s| *s > 0.0).map(Duration::from_secs_f64)
}

#[derive(Resource)]
struct Smoke {
    began: Instant,
    length: Duration,
    frames: u64,
}

/// Quits the app after the smoke run's time, when this is one.
pub fn add(app: &mut App) {
    if let Some(length) = seconds() {
        eprintln!("smoke: running for {:.0} seconds", length.as_secs_f64());
        app.insert_resource(Smoke { began: Instant::now(), length, frames: 0 }).add_systems(Last, tick);
    }
}

fn tick(mut smoke: ResMut<Smoke>, mut exit: MessageWriter<AppExit>) {
    smoke.frames += 1;
    if smoke.began.elapsed() < smoke.length {
        return;
    }
    let seconds = smoke.began.elapsed().as_secs_f64();
    println!("smoke: drew {} frames in {seconds:.1} seconds", smoke.frames);
    // The first frames are spent starting up; a working build draws more.
    if smoke.frames < 10 {
        eprintln!("smoke: too few frames drawn");
        exit.write(AppExit::from_code(1));
    } else {
        exit.write(AppExit::Success);
    }
}
