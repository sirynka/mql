use mql::dsl::MQL;
use mql::song::{Songs, anyhow_songs_to_ok_err, read_songs_in_dir};

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;
    let (songs, _) = anyhow_songs_to_ok_err(songs);

    let q = MQL::parse("artist == eminem && artist.len > 1")?;
    let (songs, mql_runtime_errors) = songs.validate(&q);
    let Songs(songs) = songs.filter(&q);

    if !mql_runtime_errors.is_empty() { MQL::report(mql_runtime_errors); }
    for s in songs { println!("{:?}", s.name().unwrap()); }
    Ok(())
}
