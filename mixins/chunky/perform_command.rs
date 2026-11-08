// Server-side /chunky command execution (players and server console).
fn chunky_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_hooks::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
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
        crate::game::chunky_glue::Sender::Player(player.uuid),
        args.trim(),
    );
}
