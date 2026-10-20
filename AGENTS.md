# AGENTS.md

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/` in this repo. See
`docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `GLOSSARY.md` at the repo root and `docs/adr/`. See
`docs/agents/domain.md`.

## The user guide package

`docs/VCW-User-Guide/` holds the authored user guide: the `.docx`, the `.pdf`,
the `.md` source, and the `assets/` and `screenshots/` folders they reference.

**It is editable, but not casually.** The author wrote it in a particular
voice and the rules that hold that voice are in `docs/agents/user-guide-voice.md`:
read them before touching a word, and follow the mechanics section rather than
round-tripping the Word document through an editor. An edit that can be
identified as an edit is a bad edit whatever it says. A typo or a sentence that
reads badly is still the author's to fix: say what and why and leave it alone.
The `.docx` is the golden record, the `.md` carries the identical sentences, and
the `.pdf` is rebuilt from the `.docx`. All three move together or none of them
move, and so does a figure: the `assets/` diagrams are drawn by hand and are
the author's, but the screenshots are shots of this application and go stale
silently, so they are retaken whenever the window they show changes. The voice
document holds the recipe, and the rule that every chapter starts a page with
nothing left stranded on the next one.

One sentence is nobody's to write by hand: the version the guide was prepared
against, which is a fact about the release rather than a piece of writing.
`tools/guide-version.py` owns it: it restamps the `.md` and the `.docx` and
rebuilds the `.pdf` from the `.docx`, copying every other byte of the Word
document across untouched. **Run it as part of every version bump**, in the
same commit as the four manifests, and never restamp the edition line by hand
in one format.

It replaced `docs/USER-GUIDE.md`, which is gone, so three gate tests now read
it instead: `crates/cli/tests/the_user_guide_names_real_commands.rs` requires
every `vcw` verb to be documented and every command line in the guide to be one
the CLI accepts, and `app/src-tauri/tests/the_user_guide_lists_every_key.rs`
requires every shortcut it prints to be one `app/ui/src/keymap.ts` binds. Read
the failure before reaching for the guide: a renamed flag is a question about
the rename, and a shortcut the guide prints that the keymap does not bind is a
keymap bug more often than a documentation one. The third,
`the_guide_names_the_version_it_was_prepared_against`, has only ever had one
fix: run `tools/guide-version.py`.

`docs/TRANSLATING.md` is the one piece of the old guide that was kept in the
repository, because the new one does not cover writing a catalog. That file is
ordinary documentation and is mine to maintain.

## Credentials

**Never put a secret variable where a shell can expand it.** Not in an `echo`,
a `grep`, a test, a heredoc or argv. This is a rule about the construct, not
about the intent: `${VAR:-unset}` and `${VAR:="none"}` print the value, and

```sh
echo "GH_TOKEN=${GH_TOKEN:+SET}${GH_TOKEN:-unset}"   # prints the token
```

reads as a set-or-unset report while being a `cat` of the secret. `:+` is safe,
`:-` and `:=` are not, and the pair written together is how `GH_TOKEN` reached
a session transcript twice on 2026-10-18.

**First ask whether the variable needs looking at.** `gh auth status` reports
the account, the scopes and whether auth works, without the token being
expanded at all. Most credentials have a verb like that.

**If existence really is the question:**

```sh
env | grep GH_TOKEN | wc -l      # 1 if set, 0 if not
```

The `wc -l` is doing the work. Without it the same line prints the value, so
the pipe is not optional and `grep -c` is the safer spelling of it.
`${#VAR}` is a length and is safe to print.

**To push**, let git ask the credential helper rather than reading a token:

```sh
git -c credential.helper='!gh auth git-credential' push
```
