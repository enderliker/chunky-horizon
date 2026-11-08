// World lifecycle: auto-resumes paused tasks on open and saves them on close.
fn chunky_lifecycle(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::WorldPhase,
) {
    match phase {
        crate::game::native_lifecycle::WorldPhase::WorldOpenReady => {
            crate::game::chunky_glue::on_world_open(game);
        }
        crate::game::native_lifecycle::WorldPhase::WorldCloseFinal
        | crate::game::native_lifecycle::WorldPhase::WorldCloseFailed => {
            crate::game::chunky_glue::on_world_close(game);
        }
        _ => {}
    }
}
