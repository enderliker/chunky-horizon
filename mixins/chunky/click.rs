// Handles clicks in Options > Mods > Chunky.
fn chunky_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    if mod_id != chunky_core::ID {
        return;
    }
    crate::game::chunky_glue::settings_click(game, button);
}
