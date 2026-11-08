// Client-side /chunky command handler.
fn chunky_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !game.native_on(chunky_core::ID) {
        return;
    }
    let Some(args) = command.strip_prefix("chunky") else {
        return;
    };
    if !(args.is_empty() || args.starts_with(' ')) {
        return;
    }
    *ran = true;
    crate::game::chunky_glue::handle_command(
        game,
        crate::game::chunky_glue::Sender::Client,
        args.trim(),
    );
}
