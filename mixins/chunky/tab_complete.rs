// Injects native_tab_complete_v2 to handle tab completion for /chunky.
fn chunky_tab_complete(
    label: &str,
    arguments: &[String],
    range: [usize; 2],
    suggestions: &mut Vec<horizon_client::command::Suggestion>,
) {
    if label != chunky_core::ID {
        return;
    }
    crate::game::chunky_glue::handle_tab_complete(arguments, range, suggestions);
}
