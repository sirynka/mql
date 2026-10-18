use maud::{Markup, html};
use mql::mql::MqlError;
use mql::song::Song;

pub fn songs(songs: &[&Song]) -> Markup {
    html! {
        p { (songs.len()) " songs" }
        table {
            @for s in songs {
                tr { td { (s.name().unwrap_or("?")) } }
            }
        }
    }
}

pub fn error(e: &MqlError) -> Markup {
    html! { pre.error { (e) } }
}
