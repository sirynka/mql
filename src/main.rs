use std::path::{Path, PathBuf};
use itertools::Itertools;

type Tags = metaflac::block::VorbisComment;
// type Tags = HashMap<String, Vec<String>>;

#[derive(Debug)]
pub enum Lyrics {
    Synced(Vec<(String, String)>), // .lrc
    Unsynced(Vec<String>),         // .nlrc
}

#[derive(Debug)]
pub struct Song {
    pub path: PathBuf,
    pub lyrics: Option<Lyrics>,
    pub tags: Option<Tags>,
}

fn read_song_lyrics(flac_path: &Path) -> Option<Lyrics> {
    let lrc_path = flac_path.with_extension("lrc");
    let nlrc_path = flac_path.with_extension("nlrc");

    if lrc_path.exists() {
        let synced = std::fs::read_to_string(lrc_path)
            .ok()?
            .lines()
            .filter_map(|line| {
                let (time, text) = line.split_at(10); // i don't care
                Some((time.to_string(), text.to_string()))
            })
            .collect_vec();

        return Some(Lyrics::Synced(synced));
    }

    if nlrc_path.exists() {
        let unsynced = std::fs::read_to_string(nlrc_path)
            .ok()?
            .lines()
            .map(|line| line.to_string())
            .collect_vec();

        return Some(Lyrics::Unsynced(unsynced));
    }

    None
}

fn read_song_from_glob(path: glob::GlobResult) -> anyhow::Result<Song> {
    let flac_path = path?;
    let tags = metaflac::Tag::read_from_path(&flac_path)?
        .vorbis_comments().cloned();

    Ok(Song {
        lyrics: read_song_lyrics(&flac_path),
        path: flac_path,
        tags,
    })
}

// (Vec<Song>, Vec<Err>) - ?
fn read_songs_in_dir(dir: &str) -> anyhow::Result<Vec<anyhow::Result<Song>>> {
    let flac_path = std::path::Path::new(dir).join("*.flac");
    let flac_glob = flac_path.to_str().ok_or(anyhow::anyhow!("to_str({dir}/*.flac) is fucked"))?;
    let flac_list = glob::glob(flac_glob)?;
    Ok(flac_list.map(read_song_from_glob).collect_vec())
}

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;

    let s = &songs[0].as_ref().unwrap().tags;
    println!("{s:#?}");

    Ok(())
}
