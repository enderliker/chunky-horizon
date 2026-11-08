// Dedicated server console command execution (`chunky ...` or `/chunky ...` typed in terminal).
fn chunky_perform_console_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !game.native_on(chunky_core::ID) {
        return;
    }
    let line = command.strip_prefix('/').unwrap_or(command);
    let Some(args) = line.strip_prefix("chunky") else {
        return;
    };
    if !(args.is_empty() || args.starts_with(' ')) {
        return;
    }
    *ran = true;
    crate::game::chunky_glue::handle_command(
        game,
        crate::game::chunky_glue::Sender::Console,
        args.trim(),
    );
}
