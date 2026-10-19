# AGENTS.md

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/` in this repo. See
`docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `GLOSSARY.md` at the repo root and `docs/adr/`. See
`docs/agents/domain.md`.

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
