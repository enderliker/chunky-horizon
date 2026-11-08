// Adds Chunky to Options > Mods.
fn chunky_list(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::native_hooks::NativeMod {
        id: chunky_core::ID.to_owned(),
        title: format!("{} {}", chunky_core::NAME, chunky_core::VERSION),
        description: chunky_core::DESCRIPTION.to_owned(),
        keys: Vec::new(),
    });
}
