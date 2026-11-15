# Shell completion and sessions

`keycloak-cli` completes dynamically: the shell asks the binary, the binary answers.
That keeps completion in step with the installed build, and it is what allows live
values — the realms, users, groups and clients of the session you logged in with.

This file documents how it works, what it stores, and what to do when it does not.

## Registering completion

```sh
keycloak-cli init bash        # eval "$(keycloak-cli init bash)"     in ~/.bashrc
keycloak-cli init zsh         # source <(keycloak-cli init zsh)      in ~/.zshrc
keycloak-cli init fish        # COMPLETE=fish keycloak-cli | source  in ~/.config/fish/completions/
keycloak-cli init elvish
keycloak-cli init powershell
```

`init` prints two lines with the absolute path of the binary:

1. `export KEYCLOAK_CLI_SESSION="$(… session-id)"` — names this terminal (see below);
2. the completion registration for that shell.

Bash uses `eval "$(…)"` rather than `source <(…)` because macOS still ships bash
3.2, where `source <(…)` defines the completion function in a subshell and TAB
silently does nothing.

**Re-source after an upgrade or a move.** The shell↔binary completion protocol is
documented as unstable by `clap`, and the generated shell code calls the binary by
the path it had when `init` ran. Running `init` once and writing the output into a
file is the one thing that breaks later; running it on every shell start is
self-correcting.

## Sessions

A session is one terminal's login:

```sh
keycloak-cli login      # password once
keycloak-cli clients list   # no password, no token request per command
keycloak-cli logout
```

- `keycloak-cli session-id` prints the id of the terminal: `tty:/dev/pts/3` when
  there is a terminal, `pid:<pid>-<nanoseconds>` otherwise. `init` exports it as
  `KEYCLOAK_CLI_SESSION`, and completion subprocesses inherit it. Two shells on the
  same terminal share an id (and so a session), which is the same trade-off `sudo`
  makes with tty-keyed timestamps.
- `login` stores the token in `$XDG_STATE_HOME/keycloak-cli/sessions.json`
  (`~/.local/state/...` by default), file mode `0600` in a `0700` directory,
  together with the server, realm, user, `expires_at` and `created_at`.
  `KEYCLOAK_CLI_STATE_DIR` moves the directory (sandboxes, tests).
- A stored token is used until it expires; after that the command asks for a
  password again unless you log in. Expired records are pruned on the next login.
- Another terminal, or a shell that never ran `init`, has no session: commands need
  a password and completion offers no live values.

Losing the session is not a security event — it is a cache. Deleting the state file
or running `logout` is always safe.

## Live values

With a session, completion offers:

| parameter | values | endpoint |
|-----------|--------|----------|
| `--realm`, `-r` | realm names | `GET /admin/realms` |
| user parameters (`users get <user-id>`, …) | user names | `GET /admin/realms/{realm}/users` |
| group parameters | group ids | `GET /admin/realms/{realm}/groups` |
| client parameters (`clients get <client-uuid>`, …) | client ids | `GET /admin/realms/{realm}/clients` |

The realm is the session's realm: a completion callback sees only the word being
completed, not the other flags, so `-r other` earlier on the line does not change
what is offered. User *names* are offered because Keycloak accepts a user name
where it accepts an id; groups and clients are addressed by internal id, so their
ids are what is offered.

Values are listed with `max=100` and brief representations, and the source only
exists when the build enabled the tag whose method it calls.

### Budget

One TAB is one process, so every lookup is bounded:

- answers are cached on disk for 60 s in
  `$XDG_STATE_HOME/keycloak-cli/completion/`, keyed by a hash of the session id, so
  a second TAB on the same terminal makes no request;
- a source call has a 1.2 s timeout (it runs on a worker thread; a stuck server
  cannot hang the shell);
- at most 100 candidates are printed, filtered by what was typed;
- any failure — no session, an expired token, a 500, a timeout — yields no
  candidates and prints nothing.

Measured against a local stub in a debug build: cold TAB 42 ms, warm TAB 38 ms with
zero requests, an API that answers after 3 s cut off at 1262 ms with no candidates
and no stderr.

### Turning it off

```sh
export KEYCLOAK_CLI_NO_LIVE_COMPLETION=1
```

Live values are skipped entirely; commands and flags still complete.

## Security notes

- Tokens live on disk only in the session file, readable by the owner alone. They
  are never printed: `login` reports the session id, server, realm, user and
  expiry, and `Debug` for a session shows `<redacted>`.
- The completion cache holds names and ids, not secrets, and is readable by the
  owner only. It is safe to delete.
- A completion subprocess never asks for a password: without a session it offers no
  values. Passwords belong on the command line or in `KEYCLOAK_PASSWORD`, not in a
  TAB.
- The session is a terminal-scoped convenience, not an agent: an `ssh-agent`-style
  socket that other processes cannot read is a possible later upgrade; the state
  file plus the exported id is what ships today.

## Troubleshooting

| symptom | cause |
|---------|-------|
| TAB completes commands but no values | no session in that shell: run `keycloak-cli login` (and check `echo $KEYCLOAK_CLI_SESSION`) |
| a new terminal offers nothing | expected: each terminal has its own session |
| values are stale | the on-disk cache lives 60 s; delete `$XDG_STATE_HOME/keycloak-cli/completion` |
| nothing completes at all | the registration is stale (binary moved or upgraded): re-run `init` |
| nushell completes nothing dynamic | not supported by the dynamic engine; use a static generator |

`KEYCLOAK_CLI_STATE_DIR` and `KEYCLOAK_CLI_TTY` exist for tests and sandboxes;
`KEYCLOAK_CLI_SESSION` is what `init` exports.
