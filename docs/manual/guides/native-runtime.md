# Native runtime

A browser keeps the console, crash messages, local storage and TLS for the page. A native program has to bring its own. These modules have no SHAG source; each says so at the top of its file.

Source: `crates/shagger/src/core/`, `crates/shagger/src/network/client/tls.rs`.

## Crash reports

`core::crash_report::install(program, version, dir)` sets a panic hook. Before the default message, a panic writes `crash-<seconds since 1970>.txt` into `dir`: the program and its version, the system, the thread, the message, where it was raised and a backtrace. Only the newest 20 reports are kept (`crash_report::KEEP`). A panic on a worker thread ends that worker, not the game.

## The console log

A game started from its icon has no terminal, so what it prints would be lost. `core::console_log::redirect(path)` points standard output and standard error at a file when standard output is not a terminal (a Windows GUI build, or an AppImage or app bundle started from the desktop). The file is new each run, and the run before is kept beside it as `<stem>.old.<ext>`.

## Whole-file writes

`core::atomic_file::write_atomic(path, bytes)` writes to `<name>.part`, flushes it to disk and renames it over the file, so a crash or power loss mid write leaves the old file, never a cut-off one. The browser's `localStorage` gives the same guarantee.

`write_atomic_later` does the same on a writer thread, so the caller never waits for the disk: a slider sets its cvar every frame, and a flush can take tens of milliseconds. A newer write to the same path replaces one not yet started. Call `flush_pending(limit)` before the process exits. The cvar file is saved this way.

`write_atomic_private` is the same for a secret, such as the player's account key: on Unix the file is created readable and writable by its owner only (0600), where the usual umask would leave it readable by every user of the machine. `restrict_to_owner(path)` tightens a file written before that. A failed write removes its part file.

## Bounded joins

A network worker can be stuck in a name lookup the operating system will not cut short (an offline machine, a captive portal). Joining it from a `Drop` on the game thread would freeze the window for as long. `core::join::join_within(thread, limit)` waits up to `limit`, then leaves the thread to end on its own. The game and lobby connections use it when they are dropped, with a limit of 1.5 seconds.

## TLS

`wss://` connections to the lobby and game servers, and HTTPS downloads, go through rustls with the `ring` provider. The client trusts the Mozilla roots bundled by `webpki-roots`, so it behaves the same on every machine, whatever the system's certificate store holds.

Like Node's `NODE_EXTRA_CA_CERTS`, the PEM certificates in the file named by `SSL_CERT_FILE` are trusted as well, for networks that re-sign TLS through a proxy of their own. A file that cannot be read adds nothing, and the bundled roots still apply.

HTTP(S) downloads (`network::client::http_fetch`) follow at most ten redirects, and never from `https://` to plain `http://`, so packages and updates arrive over TLS or not at all. `HttpFetch::start_at_most` refuses a body past a size as soon as it passes it, so a wrong or hostile server cannot fill the memory.

Both client WebSockets, the lobby's and the game server's, accept messages and frames up to `network::client::tls::MAX_MESSAGE_BYTES` (4 MiB) instead of tungstenite's 64 MiB. A larger message ends the connection as an error (code 1006). The largest real messages, board pages and snapshots, are tens of kilobytes.
