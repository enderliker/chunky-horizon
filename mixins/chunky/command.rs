// Client-side /chunky command handler.
fn chunky_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !game.native_on(chunky_core::ID) {
        return;
    }
    // In integrated single-player and remote/LAN multiplayer, commands must not be claimed
    // on the client, allowing Horizon to forward them via chat packets to the authoritative server.
    if !chunky_core::command::should_claim_on_client(game.remote.is_some(), command) {
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
        crate::game::chunky_glue::Sender::Client,
        args.trim(),
    );
}
