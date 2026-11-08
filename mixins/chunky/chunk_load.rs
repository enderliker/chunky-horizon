// Called whenever a chunk loads or finishes generating on the server.
fn chunky_chunk_load(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    chunk: [i32; 2],
    new: bool,
) {
    crate::game::chunky_glue::on_chunk_load(game, dimension, chunk, new);
}
