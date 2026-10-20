# Translating VCW

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

VCW ships in US English and is translated by the people who use it. The whole
catalog is one file:

1. Copy [`i18n/en-US.toml`](../i18n/en-US.toml) out of the repository to
   `<your IETF language tag>.toml` - `de-DE`, `pt-BR`, `ja-JP`.
2. Replace each `text` with your language. Leave the keys alone.
3. Give each entry a `source` field holding the eight hex digits that
   `vcw doctor --i18n` prints for its key. That records which English you
   translated, so VCW can tell you later which entries went stale because the
   English changed underneath them.
4. Put the file in an `i18n` directory beside your settings file - see
   [Where your settings live](#where-your-settings-live) - and reopen
   Settings. Your language is in the menu.

Anything you have not translated stays in English rather than going blank, so a
half-finished catalog is usable from the first entry. When you are happy with
it, send it in as a pull request and it ships with the next release.

Two things to know while you work:

- **A `{name}` in braces is a value VCW puts in.** Move it wherever your
  language wants it and leave the spelling alone; a name VCW does not
  recognize is left on screen as itself, which is how you spot a typo.
- **In the Settings panel, a backtick marks a literal and an asterisk marks an
  emphasis.** `` `i18n/en-US.toml` `` comes out as a file name in a monospace
  face and `*Refuse*` as the name of an option. Each paragraph is one entry
  rather than several, so you can put the sentence in whatever order your
  language puts it.

**What is not in the catalog**, and will not be: the reports `vcw` prints on
the command line, and the event log in the window. Those are column-aligned
tables and one-line event records that get piped into other programs, grepped,
and pasted into bug reports, so they stay in the source language the way a log
file does. What is in the file you just copied is the window - every panel,
every button, every hint - and the refusals VCW gives when it will not export
a capture into a format that cannot carry it, which the command line prints in
your language too.

## Where your settings live

The `i18n` directory goes beside your settings file:

| Platform | Where |
| --- | --- |
| Linux | `~/.config/dev.vcw.workstation/` |
| macOS | `~/Library/Application Support/dev.vcw.workstation/` |
| Windows | `%APPDATA%\dev.vcw.workstation\` |
