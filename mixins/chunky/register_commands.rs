// Registers server commands for Chunky.
fn chunky_register_commands(commands: &mut Vec<horizon_client::mod_api::NativeCommand>) {
    commands.push(horizon_client::mod_api::NativeCommand::new("chunky"));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky help [page:integer]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky start [world:word?] [shape:word?] [center_x:number] [center_z:number] [radius_x:number] [radius_z:number]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky pause [world:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky continue [world:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky cancel [world:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky world [world:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky worldborder [world:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky center <x:number> <z:number>",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new("chunky spawn"));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky radius <radius:number> [radius_z:number]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky corners <x1:number> <z1:number> <x2:number> <z2:number>",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky shape [shape:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky pattern [pattern:word?] [value:word]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new("chunky silent"));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky quiet [interval:integer]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky limit [rate:integer]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky trim [world:word?] [shape:word?] [center_x:number] [center_z:number] [radius_x:number] [radius_z:number] [mode:word?] [filters:text?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky selection",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky progress",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky border [args:text?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky reload [type:word?]",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky debug map",
    ));
    commands.push(horizon_client::mod_api::NativeCommand::new(
        "chunky confirm",
    ));
}
