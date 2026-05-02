//! AT Protocol richtext facet builder. Bluesky doesn't auto-detect hashtags
//! or URLs in `text` — they only render as clickable when the post record
//! includes a matching `facets` entry with UTF-8 byte offsets.

use ipld_core::ipld::Ipld;
use std::collections::BTreeMap;

const TAG_FEATURE: &str = "app.bsky.richtext.facet#tag";
const LINK_FEATURE: &str = "app.bsky.richtext.facet#link";

#[derive(Debug, Clone, PartialEq)]
struct ByteSpan {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum FeatureKind {
    Tag(String),
    Link(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Facet {
    span: ByteSpan,
    kind: FeatureKind,
}

/// Scan `text` for `#hashtag` and `http(s)://` URL spans, returning a richtext
/// facets list ready to drop into a `app.bsky.feed.post` record. Returns
/// `None` when there are no spans, so callers can omit the field entirely.
pub fn build_facets(text: &str) -> Option<Ipld> {
    let mut facets: Vec<Facet> = Vec::new();
    facets.extend(find_hashtags(text));
    facets.extend(find_urls(text));
    if facets.is_empty() {
        return None;
    }
    facets.sort_by_key(|f| f.span.start);
    Some(Ipld::List(facets.into_iter().map(facet_to_ipld).collect()))
}

fn facet_to_ipld(f: Facet) -> Ipld {
    let feature = match f.kind {
        FeatureKind::Tag(t) => Ipld::Map(BTreeMap::from([
            ("$type".into(), Ipld::String(TAG_FEATURE.into())),
            ("tag".into(), Ipld::String(t)),
        ])),
        FeatureKind::Link(u) => Ipld::Map(BTreeMap::from([
            ("$type".into(), Ipld::String(LINK_FEATURE.into())),
            ("uri".into(), Ipld::String(u)),
        ])),
    };
    Ipld::Map(BTreeMap::from([
        (
            "index".into(),
            Ipld::Map(BTreeMap::from([
                ("byteStart".into(), Ipld::Integer(f.span.start as i128)),
                ("byteEnd".into(), Ipld::Integer(f.span.end as i128)),
            ])),
        ),
        ("features".into(), Ipld::List(vec![feature])),
    ]))
}

fn find_hashtags(text: &str) -> Vec<Facet> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        let prev_ok = i == 0 || !is_tag_char(bytes[i - 1]);
        if !prev_ok {
            i += 1;
            continue;
        }
        let tag_start = i + 1;
        let mut j = tag_start;
        while j < bytes.len() && is_tag_char(bytes[j]) {
            j += 1;
        }
        if j > tag_start {
            let tag_text = std::str::from_utf8(&bytes[tag_start..j]).unwrap_or("");
            // Skip pure-numeric tags — Bluesky rejects them.
            if tag_text.chars().any(|c| !c.is_ascii_digit()) {
                out.push(Facet {
                    span: ByteSpan { start: i, end: j },
                    kind: FeatureKind::Tag(tag_text.to_string()),
                });
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn is_tag_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find_urls(text: &str) -> Vec<Facet> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let needles: &[&[u8]] = &[b"https://", b"http://"];
    let mut i = 0;
    while i < bytes.len() {
        let mut matched_len = 0;
        for n in needles {
            if i + n.len() <= bytes.len() && &bytes[i..i + n.len()] == *n {
                let prev_ok = i == 0
                    || matches!(
                        bytes[i - 1],
                        b' ' | b'\t' | b'\n' | b'\r' | b'(' | b'[' | b'<'
                    );
                if prev_ok {
                    matched_len = n.len();
                    break;
                }
            }
        }
        if matched_len == 0 {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + matched_len;
        while end < bytes.len() && !is_url_terminator(bytes[end]) {
            end += 1;
        }
        while end > start + matched_len {
            let c = bytes[end - 1];
            if matches!(
                c,
                b'.' | b',' | b'!' | b'?' | b';' | b':' | b')' | b']' | b'}' | b'\'' | b'"'
            ) {
                end -= 1;
            } else {
                break;
            }
        }
        let uri = std::str::from_utf8(&bytes[start..end])
            .unwrap_or("")
            .to_string();
        if !uri.is_empty() {
            out.push(Facet {
                span: ByteSpan { start, end },
                kind: FeatureKind::Link(uri),
            });
        }
        i = end.max(i + 1);
    }
    out
}

fn is_url_terminator(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | b'<' | b'>' | b'"' | b'\'' | b')'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Vec<Facet> {
        let mut v = find_hashtags(text);
        v.extend(find_urls(text));
        v.sort_by_key(|f| f.span.start);
        v
    }

    #[test]
    fn detects_hashtags_and_url_in_norppa_message() {
        let msg = "Norppa on kivellä! Kaikki katsomaan!\n\n#norppalive #luontolive\nhttps://wwf.fi/luontolive/norppalive/";
        let facets = extract(msg);
        assert_eq!(facets.len(), 3);
        assert_eq!(
            facets[0].kind,
            FeatureKind::Tag("norppalive".into())
        );
        assert_eq!(
            facets[1].kind,
            FeatureKind::Tag("luontolive".into())
        );
        assert_eq!(
            facets[2].kind,
            FeatureKind::Link("https://wwf.fi/luontolive/norppalive/".into())
        );
        // Verify byte offsets recover the original substrings.
        for f in &facets {
            let slice = &msg.as_bytes()[f.span.start..f.span.end];
            let s = std::str::from_utf8(slice).unwrap();
            match &f.kind {
                FeatureKind::Tag(t) => assert_eq!(s, format!("#{}", t)),
                FeatureKind::Link(u) => assert_eq!(s, u),
            }
        }
    }

    #[test]
    fn ignores_hash_in_word_middle() {
        // word#tag should not be treated as a hashtag — `#` follows a tag char.
        let facets = extract("foo#bar baz");
        assert!(facets.is_empty());
    }

    #[test]
    fn ignores_pure_numeric_hashtags() {
        let facets = extract("price #123 tag");
        assert!(facets.is_empty());
    }

    #[test]
    fn strips_trailing_punctuation_from_url() {
        let msg = "see https://example.com/path. end";
        let facets = extract(msg);
        assert_eq!(facets.len(), 1);
        assert_eq!(
            facets[0].kind,
            FeatureKind::Link("https://example.com/path".into())
        );
    }

    #[test]
    fn handles_utf8_multibyte_offsets() {
        // 'ä' is 2 bytes in UTF-8. Hashtag offset must be byte-based.
        let msg = "kivellä #norppa";
        let bytes = msg.as_bytes();
        let facets = extract(msg);
        assert_eq!(facets.len(), 1);
        let span = &facets[0].span;
        assert_eq!(&bytes[span.start..span.end], b"#norppa");
    }

    #[test]
    fn empty_input_returns_no_facets() {
        assert!(build_facets("").is_none());
        assert!(build_facets("plain text with no tags").is_none());
    }

    #[test]
    fn build_facets_sorted_and_typed() {
        let ipld = build_facets("a https://x.io/y #tag b").expect("facets");
        match ipld {
            Ipld::List(items) => {
                assert_eq!(items.len(), 2);
                // First by byteStart should be the URL.
                if let Ipld::Map(m) = &items[0] {
                    if let Some(Ipld::List(features)) = m.get("features") {
                        if let Ipld::Map(feat) = &features[0] {
                            match feat.get("$type") {
                                Some(Ipld::String(t)) => {
                                    assert_eq!(t, "app.bsky.richtext.facet#link")
                                }
                                _ => panic!("expected $type string"),
                            }
                        }
                    }
                }
            }
            _ => panic!("expected list"),
        }
    }
}
