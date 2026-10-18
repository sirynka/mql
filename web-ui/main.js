let seq = 0 // Claude: `seq` drops responses that arrive after a newer one
let push_next_query = false

let debounce_timer
let history_update_timeout

query.oninput = () => {
  clearTimeout(debounce_timer)
  clearTimeout(history_update_timeout)

  debounce_timer = setTimeout(async () => {
    const my = ++seq
    const res = await fetch("/q?" + new URLSearchParams({ q: query.value }))
    const html = await res.text()
    if (my !== seq) return

    output.innerHTML = html
    if (!res.ok) return

    // Claude: the address bar follows every query that parses. A query is kept in history once you
    // paused on it for a second and it found songs, the next one then gets a new history entry.
    const url = "#" + new URLSearchParams({ q: query.value })
    if (push_next_query) history.pushState(null, "", url)
    else                 history.replaceState(null, "", url)
    push_next_query = false

    if (output.querySelector("tr")) history_update_timeout = setTimeout(() => push_next_query = true, 1000)
  }, 100)
}

// Claude: on load and on back/forward, the query comes from the url
const load = () => {
  push_next_query = false
  query.value = new URLSearchParams(location.hash.slice(1)).get("q") ?? ""
  query.oninput()
}

window.onpopstate = load
load()
