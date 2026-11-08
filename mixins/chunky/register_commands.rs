// Registers server commands for Chunky.
fn chunky_register_commands(commands: &mut Vec<horizon_client::mod_api::NativeCommand>) {
    commands.push(horizon_client::mod_api::NativeCommand::new("chunky"));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky [args:text]",
    ));
}
