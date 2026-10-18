use mql::mql::MQL;
use mql::song::{Songs, anyhow_songs_to_ok_err, read_songs_in_dir};

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;
    let (songs, _) = anyhow_songs_to_ok_err(songs);

    let q = r#"lyrics.len == 0"#;
    println!("{q}");
    let q = MQL::parse(q)?;

    let Songs(songs) = songs.filter(&q);
    let song_s = if songs.len() == 1 { "song" } else { "songs" };
    println!("{} {song_s}", songs.len());
    println!();
    for s in songs { println!("{}", s.name().unwrap()); }
    Ok(())
}
