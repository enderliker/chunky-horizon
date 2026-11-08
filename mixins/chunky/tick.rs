// Drives pre-generation tasks during the authoritative server tick.
fn chunky_server_tick(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::ServerTickPhase,
) {
    if matches!(
        phase,
        crate::game::native_lifecycle::ServerTickPhase::ServerTickEnd { .. }
    ) {
        crate::game::chunky_glue::tick(game);
    }
}
