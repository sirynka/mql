use mql::dsl::MQL;
use mql::song::{Songs, anyhow_songs_to_ok_err, read_songs_in_dir};

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;
    let (songs, _) = anyhow_songs_to_ok_err(songs);

    // let q = MQL::parse("artist == eminem && artist.len > 1")?;

    let q = MQL::parse(r#"mbz_tag.str contains 'hip hop' and mbz_tag.str does not contain rap"#)?;

    let Songs(songs) = songs.filter(&q);
    for s in songs { println!("{:?}", s.name().unwrap()); }
    Ok(())
}
