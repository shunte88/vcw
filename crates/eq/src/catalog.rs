/*
 *  catalog.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The shipped set of curves, and the user file that extends it.
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! The curve catalog.
//!
//! The built-in set is `curves.toml`, compiled in. A user file of the same shape
//! extends it, and an id that collides replaces the built-in entry - which is how §51's
//! "user-defined curve stated in the same terms" is met without a curve editor.

use serde::Deserialize;

use crate::Error;
use crate::curve::Curve;

/// The shipped catalog, as text. Exposed so a user writing their own file has
/// something to copy, and so `vcw` can print it.
pub const BUILTIN_TOML: &str = include_str!("../curves.toml");

/// The id of the curve every post-1954 record was cut to.
pub const RIAA: &str = "riaa";

/// The id of the entry that applies no equalization.
pub const FLAT: &str = "flat";

/// The characteristic in force, from the three levels §51 allows it to be stated at.
///
/// "Selectable per side, overridable per track", plus the release underneath both.
/// `None` at a level means nobody said anything there, so the question passes
/// outward; nothing stated anywhere is [`RIAA`], which is what all but a handful of
/// records in anybody's collection were cut to.
///
/// Here rather than in `vcw-project` because the default is a fact about curves, and
/// a project crate that knew which curve was usual would be the second place that
/// fact lived.
///
/// ```
/// # use vcw_eq::{in_force, RIAA};
/// assert_eq!(in_force(None, None, None), RIAA);
/// assert_eq!(in_force(None, None, Some("columbia_lp")), "columbia_lp");
/// assert_eq!(in_force(Some("aes"), Some("emi"), Some("riaa")), "aes");
/// ```
#[must_use]
pub fn in_force<'a>(
    track: Option<&'a str>,
    side: Option<&'a str>,
    release: Option<&'a str>,
) -> &'a str {
    track.or(side).or(release).unwrap_or(RIAA)
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    curve: Vec<Curve>,
}

/// A set of curves, in the order the file lists them.
///
/// Order is the curated one - RIAA first, then roughly chronological - because it is
/// the order a chooser should offer them in, and a map would lose it.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    curves: Vec<Curve>,
}

impl Catalog {
    /// The curves VCW ships.
    ///
    /// # Panics
    ///
    /// Never, in a build that passed its tests: `curves.toml` is compiled in, so a
    /// malformed catalog is a compile-time artifact and `the_builtin_catalog_parses`
    /// fails before anything ships. A `Result` here would be a lie about when the
    /// error can happen.
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_TOML).expect("the compiled-in catalog is well formed")
    }

    /// Parse a catalog from TOML, checking the invariants §51 asks for.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: File = toml::from_str(text).map_err(|source| Error::Malformed {
            source: Box::new(source),
        })?;
        let catalog = Self { curves: file.curve };
        catalog.validate()?;
        Ok(catalog)
    }

    /// This catalog with `overrides` applied: new ids appended, existing ids replaced.
    pub fn with_overrides(mut self, overrides: Self) -> Self {
        for curve in overrides.curves {
            match self.curves.iter_mut().find(|c| c.id == curve.id) {
                Some(existing) => *existing = curve,
                None => self.curves.push(curve),
            }
        }
        self
    }

    /// The curve with this id.
    pub fn get(&self, id: &str) -> Option<&Curve> {
        self.curves.iter().find(|c| c.id == id)
    }

    /// The curve with this id, or [`Error::UnknownCurve`] naming what is on offer.
    pub fn require(&self, id: &str) -> Result<&Curve, Error> {
        self.get(id).ok_or_else(|| Error::UnknownCurve {
            id: id.to_owned(),
            known: self.ids().collect::<Vec<_>>().join(", "),
        })
    }

    /// Every curve, in catalog order.
    pub fn iter(&self) -> impl Iterator<Item = &Curve> {
        self.curves.iter()
    }

    /// Every curve id, in catalog order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.curves.iter().map(|c| c.id.as_str())
    }

    /// How many curves the catalog holds.
    pub fn len(&self) -> usize {
        self.curves.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    fn validate(&self) -> Result<(), Error> {
        for (i, curve) in self.curves.iter().enumerate() {
            if curve.citation.trim().is_empty() {
                return Err(Error::MissingCitation {
                    id: curve.id.clone(),
                });
            }
            if self.curves[..i].iter().any(|c| c.id == curve.id) {
                return Err(Error::DuplicateId {
                    id: curve.id.clone(),
                });
            }
            for (field, value) in [
                ("shelf_us", curve.shelf_us),
                ("turnover_us", curve.turnover_us),
                ("treble_us", curve.treble_us),
            ] {
                if value.is_some_and(|v| !(v.is_finite() && v > 0.0)) {
                    return Err(Error::BadTimeConstant {
                        id: curve.id.clone(),
                        field,
                    });
                }
            }
            // A turnover below its own shelf inverts the curve into a bass *cut*,
            // which no characteristic does and which a typo in a user file would
            // otherwise produce silently.
            if let (Some(shelf), Some(turnover)) = (curve.shelf_us, curve.turnover_us)
                && shelf <= turnover
            {
                return Err(Error::ShelfBelowTurnover {
                    id: curve.id.clone(),
                });
            }
        }
        Ok(())
    }
}
