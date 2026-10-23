//! HTML-to-plain-text normalization for SnapGene text values.
//!
//! SnapGene stores several text values as HTML documents (for example feature
//! notes and the notes Description). The canonical model stores plain text in
//! qualifier values and in description, so this module strips tags, decodes
//! entities once, collapses whitespace and trims. The raw value is preserved
//! separately under extras.snapgene so nothing is lost (lead decision,
//! documented in docs/design/snapgene-dna.md).
//!
//! A value that does not contain a tag-like sequence is returned verbatim:
//! plain text is never rewritten.

/// Returns the plain-text form of one SnapGene text value.
pub(crate) fn plain_text(raw: &str) -> String {
    if !looks_like_html(raw) {
        return raw.to_owned();
    }
    let stripped = strip_tags(raw);
    let decoded = decode_entities_once(&stripped);
    collapse_whitespace(&decoded)
}

/// Returns whether a string contains an XML/HTML-looking tag.
fn looks_like_html(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'<' {
            let next = bytes[index + 1];
            if next.is_ascii_alphabetic() || next == b'/' || next == b'!' || next == b'?' {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// Strips tags, turning block-level tags into a space.
fn strip_tags(raw: &str) -> String {
    const BLOCK_TAGS: &[&str] = &[
        "br",
        "p",
        "div",
        "li",
        "ul",
        "ol",
        "tr",
        "td",
        "th",
        "table",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "blockquote",
        "hr",
        "html",
        "body",
        "head",
    ];
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '<' {
            output.push(character);
            continue;
        }
        let mut tag = String::new();
        let mut quote: Option<char> = None;
        let mut closed = false;
        for next in chars.by_ref() {
            if let Some(active) = quote {
                if next == active {
                    quote = None;
                }
                continue;
            }
            if next == '"' || next == '\'' {
                quote = Some(next);
                continue;
            }
            if next == '>' {
                closed = true;
                break;
            }
            tag.push(next);
        }
        if !closed {
            // Not a well-formed tag; keep the literal character and the text.
            output.push('<');
            output.push_str(&tag);
            break;
        }
        let name = tag
            .trim_start_matches('/')
            .trim_start_matches('!')
            .trim_start_matches('?')
            .split(|character: char| character.is_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if BLOCK_TAGS.contains(&name.as_str()) {
            output.push(' ');
        }
    }
    output
}

/// Decodes HTML entities once: named, decimal and hexadecimal.
fn decode_entities_once(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        if character != '&' {
            output.push(character);
            continue;
        }
        let mut entity = String::new();
        let mut terminated = false;
        for (_, next) in chars.by_ref() {
            if next == ';' {
                terminated = true;
                break;
            }
            entity.push(next);
            if entity.len() > 32 {
                break;
            }
        }
        if terminated {
            if let Some(decoded) = decode_entity(&entity) {
                output.push(decoded);
                continue;
            }
        }
        output.push('&');
        output.push_str(&entity);
        if terminated {
            output.push(';');
        }
    }
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    if let Some(number) = entity.strip_prefix('#') {
        let value = if let Some(hex) = number
            .strip_prefix('x')
            .or_else(|| number.strip_prefix('X'))
        {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            number.parse::<u32>().ok()?
        };
        return char::from_u32(value);
    }
    let decoded = match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "copy" => '\u{00a9}',
        "reg" => '\u{00ae}',
        "trade" => '\u{2122}',
        "deg" => '\u{00b0}',
        "middot" => '\u{00b7}',
        "bull" => '\u{2022}',
        _ => return None,
    };
    Some(decoded)
}

/// Collapses whitespace runs to one space and trims the result.
fn collapse_whitespace(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut pending_space = false;
    for character in raw.chars() {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        pending_space = false;
        output.push(character);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_stays_verbatim() {
        assert_eq!(plain_text("lac repressor"), "lac repressor");
        assert_eq!(plain_text("a < b"), "a < b");
    }

    #[test]
    fn strips_html_wrapper_and_inline_markup() {
        assert_eq!(
            plain_text("<html><body>the <i>lac</i> repressor</body></html>"),
            "the lac repressor"
        );
    }

    #[test]
    fn block_tags_become_spaces_and_whitespace_collapses() {
        assert_eq!(
            plain_text("<html><body>line one<br>line two<p>para</p></body></html>"),
            "line one line two para"
        );
    }

    #[test]
    fn decodes_entities_once() {
        assert_eq!(plain_text("<b>a &amp; b &lt;c&gt;</b>"), "a & b <c>");
        assert_eq!(plain_text("<html><body>&#65;&#x42;</body></html>"), "AB");
    }

    #[test]
    fn unknown_entities_are_kept_literally() {
        assert_eq!(plain_text("<b>&unknown;</b>"), "&unknown;");
    }

    #[test]
    fn empty_html_normalizes_to_empty() {
        assert_eq!(plain_text("<html><body><br></body></html>"), "");
    }
}
