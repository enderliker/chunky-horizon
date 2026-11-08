// Buttons for Options > Mods > Chunky.
fn chunky_settings(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != chunky_core::ID {
        return;
    }
    crate::game::chunky_glue::settings_buttons(game, buttons);
}
