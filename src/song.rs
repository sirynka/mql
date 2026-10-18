use std::path::{Path, PathBuf};

use itertools::Itertools;

use crate::dsl::{self, Expr, Loc};

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

impl Song {
    pub fn name(&self) -> Option<&str> {
        self.path.file_stem().and_then(|s| s.to_str())
    }

    // Claude: vorbis tag names are case-insensitive ASCII, a missing tag (or no tags at all) yields nothing
    pub fn tag(&self, name: &str) -> impl Iterator<Item = &str> {
        self.tags.iter()
            .flat_map(|tags| tags.comments.iter())
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .flat_map(|(_, values)| values.iter().map(String::as_str))
    }

    pub fn matches(&self, q: &Loc<Expr>) -> bool {
        dsl::eval(q, self)
    }
}

pub struct Songs(pub Vec<Song>);

impl<'s, 'q> Songs {
    pub fn filter(self, ast: &'q Loc<Expr<'q>>) -> Self {
        Songs(self.0.into_iter().filter(|s| s.matches(ast)).collect())
    }
}

pub fn read_song_lyrics(flac_path: &Path) -> Option<Lyrics> {
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

pub fn read_song_from_glob(path: glob::GlobResult) -> anyhow::Result<Song> {
    let flac_path = path?;
    let tags = metaflac::Tag::read_from_path(&flac_path)?
        .vorbis_comments()
        .cloned();

    Ok(Song {
        lyrics: read_song_lyrics(&flac_path),
        path: flac_path,
        tags,
    })
}

pub fn read_songs_in_dir(dir: &str) -> anyhow::Result<Vec<anyhow::Result<Song>>> {
    let flac_path = std::path::Path::new(dir).join("*.flac");
    let flac_glob = flac_path
        .to_str()
        .ok_or(anyhow::anyhow!("to_str({dir}/*.flac) is fucked"))?;
    let flac_list = glob::glob(flac_glob)?;
    Ok(flac_list.map(read_song_from_glob).collect_vec())
}

pub fn anyhow_songs_to_ok_err(songs: Vec<anyhow::Result<Song>>) -> (Songs, Vec<anyhow::Error>) {
    let (ok, err): (Vec<_>, Vec<_>) = songs.into_iter().partition(Result::is_ok);
    let ok = ok.into_iter().map(Result::unwrap).collect_vec();
    let err = err.into_iter().map(Result::unwrap_err).collect_vec();
    (Songs(ok), err)
}
