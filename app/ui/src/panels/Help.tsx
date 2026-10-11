/*
 *  Help.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The whole keyboard map, grouped by scope (§43).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Generated from `BINDINGS`, not written out again. A help overlay maintained by
// hand is a help overlay that is wrong: it says a key does something it stopped
// doing two work packages ago, and a person who trusts it stops trusting the
// rest of the application.
//
// It shows the whole map regardless of which panel is in front, and marks the
// ones that do not apply here rather than hiding them. A person pressing `?`
// wants to know what the application can do; a filtered list would answer a
// question they did not ask and would make `t` look as though it did not exist.
//
// §43's suggested defaults are marked, because they are the chords a person
// arrives already knowing.

import { key, useText } from "../i18n";
import { grouped } from "../keys";
import { spellings } from "../keymap";
import type { Scope } from "../keymap";

/** The catalog key for what each scope is called in front of a person. */
const NAMES: Record<Scope, string> = {
  global: key("help.scope.global"),
  browser: key("help.scope.browser"),
  capture: key("help.scope.capture"),
  tracks: key("help.scope.tracks"),
  metadata: key("help.scope.metadata"),
  export: key("help.scope.export"),
  settings: key("help.scope.settings"),
};

/** The keyboard map overlay. */
export function Help({
  scope,
  onClose,
}: {
  scope: Scope;
  onClose: () => void;
}): React.JSX.Element {
  const text = useText();
  return (
    <div className="overlay" role="dialog" aria-label={text("help.aria")}>
      <div className="overlay-box">
        <header className="panel-head">
          <h2>{text("help.title")}</h2>
          <button type="button" onClick={onClose}>
            {text("help.close")}
          </button>
        </header>
        {grouped().map(([group, bindings]) => (
          <section key={group} className="help-group">
            <h3>
              {text(NAMES[group])}
              {group !== "global" && group !== scope && (
                <span className="dim"> {text("help.elsewhere")}</span>
              )}
            </h3>
            <table className="rows">
              <tbody>
                {bindings.map(([action, binding]) => (
                  <tr key={action}>
                    <td className="chord">
                      {spellings(binding).map((chord) => (
                        <kbd key={chord}>{chord}</kbd>
                      ))}
                    </td>
                    <td>{text(binding.label)}</td>
                    <td className="dim">
                      {binding.suggested === true ? "§43" : ""}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        ))}
      </div>
    </div>
  );
}
