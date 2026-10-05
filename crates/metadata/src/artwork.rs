/*
 *  artwork.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Cover art: fetched, sniffed, and capped (§28).
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

//! Cover art: fetched, sniffed, and capped (§28).
//!
//! An [`ArtworkRef`] is a URL a provider gave us. [`Artwork`] is bytes that have
//! been checked. Three things are checked, and each of them is a real thing that
//! happens:
//!
//! 1. **The content type is not believed.** The Cover Art Archive redirects to
//!    Internet Archive storage and the type that comes back is sometimes
//!    `application/octet-stream`. The magic bytes are the evidence.
//! 2. **The size is capped.** A Cover Art Archive original can be 20 MB of
//!    scanned gatefold. §28 wants a cover, not an archive, so [`MAX_BYTES`] is a
//!    refusal rather than something to load and then discard.
//! 3. **It is never cached on disk by the metadata cache.** The URL cache holds
//!    JSON; images go through [`Client::uncached`] because a project that wants to
//!    keep its cover art stores it in the project, where it is part of the
//!    document and gets backed up with it.
//!
//! Nothing here decodes an image. Sniffing a format from four bytes is enough to
//! know the download is not an HTML error page, and no decoder means no decoder
//! CVE in a path that takes bytes off the internet.

use crate::client::{Cancel, Client};
use crate::error::{Error, Result};
use crate::release::ArtworkRef;

/// The largest image VCW will accept.
///
/// Eight megabytes covers a 3000x3000 JPEG comfortably and refuses a scanned
/// gatefold TIFF. A cover that will not fit is not a cover.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// The smallest thing that could be an image at all.
///
/// Below this it is an error page, a redirect body or a truncated download.
pub const MIN_BYTES: usize = 64;

/// An image format, identified from the bytes rather than from a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtworkFormat {
    /// JPEG, which is nearly always what a provider serves.
    Jpeg,
    /// PNG.
    Png,
    /// GIF, which Discogs still has a few of.
    Gif,
    /// WebP.
    WebP,
}

impl ArtworkFormat {
    /// The format of a byte slice, or `None` if it is not an image VCW knows.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Self::Jpeg);
        }
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(Self::Png);
        }
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(Self::Gif);
        }
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some(Self::WebP);
        }
        None
    }

    /// The MIME type, for a data URL or an HTTP response to the webview.
    #[must_use]
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
        }
    }

    /// The extension to write it under.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::WebP => "webp",
        }
    }
}

/// Downloaded cover art.
#[derive(Clone, PartialEq, Eq)]
pub struct Artwork {
    /// Where it came from, for attribution and for re-fetching.
    pub url: String,
    /// What it turned out to be.
    pub format: ArtworkFormat,
    /// Whether the provider called it the front cover.
    pub primary: bool,
    /// The bytes.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Artwork {
    /// Prints the shape, never the bytes.
    ///
    /// A `Vec<u8>` of eight megabytes in a debug log is not a diagnostic, it is a
    /// denial of service against whoever has to read it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Artwork")
            .field("url", &self.url)
            .field("format", &self.format)
            .field("primary", &self.primary)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl Artwork {
    /// Checks downloaded bytes and says what they are.
    ///
    /// The only way to build one, so an `Artwork` always holds something that
    /// really did start with an image's magic bytes and really does fit.
    pub fn accept(url: &str, primary: bool, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < MIN_BYTES {
            return Err(Error::NotArtwork {
                url: url.to_string(),
                detail: format!("{} bytes is too small to be an image", bytes.len()),
            });
        }
        if bytes.len() > MAX_BYTES {
            return Err(Error::NotArtwork {
                url: url.to_string(),
                detail: format!("{} bytes exceeds the {MAX_BYTES} byte limit", bytes.len()),
            });
        }
        let format = ArtworkFormat::sniff(&bytes).ok_or_else(|| Error::NotArtwork {
            url: url.to_string(),
            detail: "no image magic bytes; the server sent something else".into(),
        })?;
        Ok(Self {
            url: url.to_string(),
            format,
            primary,
            bytes,
        })
    }

    /// How big it is.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Always false: an `Artwork` cannot hold nothing.
    ///
    /// Present because clippy asks for it next to [`Artwork::len`], and honest
    /// about the fact that the answer is fixed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// A filename to write it under, given a stem.
    #[must_use]
    pub fn file_name(&self, stem: &str) -> String {
        let stem = stem.trim();
        let stem = if stem.is_empty() { "cover" } else { stem };
        format!("{stem}.{}", self.format.extension())
    }
}

/// Downloads one artwork reference.
///
/// Uncached on purpose: see the module docs. A caller that wants to keep the image
/// keeps it in the project.
pub fn fetch(client: &Client, reference: &ArtworkRef, cancel: &Cancel) -> Result<Artwork> {
    if reference.url.trim().is_empty() {
        return Err(Error::NotArtwork {
            url: String::new(),
            detail: "no URL".into(),
        });
    }
    let bytes = client.uncached(&reference.url, &[], cancel)?;
    Artwork::accept(&reference.url, reference.primary, bytes)
}

/// Downloads the front cover of a release, if it has one.
///
/// `Ok(None)` when the release names no artwork, which is an ordinary state and
/// not worth an error. A reference that fails to download *is* an error, because
/// the provider said there was a cover there.
pub fn fetch_front(
    client: &Client,
    artwork: &[ArtworkRef],
    cancel: &Cancel,
) -> Result<Option<Artwork>> {
    let Some(reference) = artwork
        .iter()
        .find(|r| r.primary)
        .or_else(|| artwork.first())
    else {
        return Ok(None);
    };
    fetch(client, reference, cancel).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::Recorded;
    use crate::net::{Offline, Response, Transport};
    use crate::policy::{Limiter, TestClock};
    use crate::release::ProviderId;
    use std::sync::Arc;

    const URL: &str = "https://coverartarchive.org/release/x/front";

    /// Bytes that start like a JPEG and are long enough to be one.
    fn jpeg(size: usize) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        bytes.resize(size, 0x20);
        bytes
    }

    fn client(transport: Arc<dyn Transport>) -> Client {
        Client::new(ProviderId::MusicBrainz, transport)
            .with_clock(Arc::new(TestClock::new()))
            .with_limiter(Limiter::unlimited())
    }

    fn reference(url: &str) -> ArtworkRef {
        ArtworkRef {
            url: url.to_string(),
            primary: true,
            width: None,
            height: None,
        }
    }

    #[test]
    fn every_format_is_recognized_by_its_magic_bytes() {
        assert_eq!(
            ArtworkFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ArtworkFormat::Jpeg)
        );
        assert_eq!(
            ArtworkFormat::sniff(b"\x89PNG\r\n\x1a\n...."),
            Some(ArtworkFormat::Png)
        );
        assert_eq!(ArtworkFormat::sniff(b"GIF89a..."), Some(ArtworkFormat::Gif));
        assert_eq!(
            ArtworkFormat::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ArtworkFormat::WebP)
        );
    }

    #[test]
    fn an_error_page_is_not_an_image_however_it_is_labeled() {
        for body in [
            &b"<!DOCTYPE html><html><body>404 Not Found</body></html>"[..],
            &b"{\"error\":\"not found\"}"[..],
            &b"RIFF\0\0\0\0WAVEfmt "[..],
        ] {
            assert_eq!(ArtworkFormat::sniff(body), None);
        }
    }

    #[test]
    fn mime_types_and_extensions_are_the_ordinary_ones() {
        assert_eq!(ArtworkFormat::Jpeg.mime(), "image/jpeg");
        assert_eq!(ArtworkFormat::Jpeg.extension(), "jpg");
        assert_eq!(ArtworkFormat::WebP.extension(), "webp");
    }

    #[test]
    fn a_real_image_is_accepted_and_describes_itself() {
        let artwork = Artwork::accept(URL, true, jpeg(1_024)).expect("a jpeg");
        assert_eq!(artwork.format, ArtworkFormat::Jpeg);
        assert_eq!(artwork.len(), 1_024);
        assert!(!artwork.is_empty());
        assert!(artwork.primary);
        assert_eq!(artwork.file_name("front"), "front.jpg");
        assert_eq!(artwork.file_name("  "), "cover.jpg");
    }

    #[test]
    fn the_debug_output_is_a_size_and_not_eight_megabytes_of_jpeg() {
        let artwork = Artwork::accept(URL, true, jpeg(4_096)).expect("a jpeg");
        let printed = format!("{artwork:?}");
        assert!(printed.contains("bytes: 4096"), "{printed}");
        assert!(
            printed.len() < 200,
            "{} characters is a log entry",
            printed.len()
        );
    }

    #[test]
    fn something_too_small_to_be_an_image_is_refused() {
        let error = Artwork::accept(URL, true, vec![0xFF, 0xD8, 0xFF]).expect_err("refused");
        assert!(matches!(error, Error::NotArtwork { .. }), "{error:?}");
        assert!(error.to_string().contains("too small"), "{error}");
    }

    #[test]
    fn a_scanned_gatefold_is_refused_by_size_before_anything_else() {
        let error = Artwork::accept(URL, true, jpeg(MAX_BYTES + 1)).expect_err("refused");
        assert!(error.to_string().contains("exceeds"), "{error}");
    }

    #[test]
    fn a_download_that_is_not_an_image_names_the_url() {
        let transport = Arc::new(Recorded::new().answering(
            URL,
            Response::ok(
                "<!DOCTYPE html><html><body>404 Not Found, sorry about that</body></html>",
            ),
        ));
        let error =
            fetch(&client(transport), &reference(URL), &Cancel::new()).expect_err("not an image");
        match error {
            Error::NotArtwork { url, detail } => {
                assert_eq!(url, URL);
                assert!(detail.contains("magic bytes"), "{detail}");
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn a_fetched_image_does_not_go_through_the_metadata_cache() {
        let transport = Arc::new(Recorded::new().answering(URL, Response::ok(jpeg(512))));
        let clock = Arc::new(TestClock::new());
        let cache = Arc::new(crate::cache::Memory::new(clock.clone()));
        let client = Client::new(ProviderId::MusicBrainz, transport.clone())
            .with_clock(clock)
            .with_cache(cache.clone())
            .with_limiter(Limiter::unlimited());
        let cancel = Cancel::new();
        fetch(&client, &reference(URL), &cancel).expect("an image");
        fetch(&client, &reference(URL), &cancel).expect("an image");
        assert_eq!(transport.calls(), 2);
        assert!(
            cache.is_empty(),
            "the URL cache holds JSON; an image belongs in the project"
        );
    }

    #[test]
    fn the_front_cover_is_preferred_over_whatever_is_first() {
        let back = "https://img/back.jpg";
        let transport = Arc::new(
            Recorded::new()
                .answering(URL, Response::ok(jpeg(512)))
                .answering(back, Response::ok(jpeg(256))),
        );
        let references = vec![
            ArtworkRef {
                url: back.to_string(),
                primary: false,
                width: None,
                height: None,
            },
            reference(URL),
        ];
        let artwork = fetch_front(&client(transport), &references, &Cancel::new())
            .expect("a fetch")
            .expect("a cover");
        assert_eq!(artwork.url, URL);
        assert_eq!(artwork.len(), 512);
    }

    #[test]
    fn a_release_with_no_artwork_is_not_an_error() {
        let found = fetch_front(&client(Arc::new(Offline)), &[], &Cancel::new()).expect("a fetch");
        assert!(found.is_none(), "and no request was attempted");
    }

    #[test]
    fn an_empty_url_is_refused_without_a_request() {
        let transport = Arc::new(Recorded::new());
        let error = fetch(&client(transport.clone()), &reference("  "), &Cancel::new())
            .expect_err("refused");
        assert!(matches!(error, Error::NotArtwork { .. }), "{error:?}");
        assert_eq!(transport.calls(), 0);
    }

    #[test]
    fn offline_artwork_is_an_offline_error_not_a_format_error() {
        let error = fetch(&client(Arc::new(Offline)), &reference(URL), &Cancel::new())
            .expect_err("offline");
        assert!(matches!(error, Error::Offline { .. }), "{error:?}");
    }
}
