//! The final file name: legal on Windows, inside the path budget, never replacing a file.

/// Names Windows reserves for devices in every folder, whatever follows the first dot.
const RESERVED: [&str; 30] = [
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "com¹", "com²", "com³", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
    "lpt6", "lpt7", "lpt8", "lpt9", "lpt¹", "lpt²", "lpt³",
];

/// How many numbered names to try before giving up on a crowded folder.
const MAX_COPIES: u32 = 999;

/// The longest path Windows 8 accepts, in UTF-16 units, without the terminating NUL.
pub const MAX_PATH_UNITS: usize = 259;

/// The file name for `title`, as `stem.ext` or `stem (2).ext` when `taken` says the plain one
/// exists, with at most `max_units` UTF-16 units. `fallback` stands in for a title with nothing
/// usable left in it. `None` when even a one-letter stem doesn't fit, or every copy is taken.
#[must_use]
pub fn output_name(
    title: &str,
    fallback: &str,
    ext: &str,
    max_units: usize,
    taken: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut stem = clean(title);
    if stem.is_empty() {
        stem = clean(fallback);
    }
    if stem.is_empty() {
        stem.push_str("video");
    }
    if let Some(end) = reserved_device(&stem) {
        stem.insert(end, '_');
    }
    let ext_units = ext.encode_utf16().count() + 1;
    for copy in 1..=MAX_COPIES {
        let suffix = if copy == 1 {
            String::new()
        } else {
            format!(" ({copy})")
        };
        let room = max_units.checked_sub(ext_units + suffix.len())?;
        let mut cut = truncate(&stem, room);
        // A cut can land just before the underscore and bring the device word back.
        if reserved_device(cut).is_some() {
            cut = truncate(cut, cut.encode_utf16().count().saturating_sub(1));
        }
        if cut.is_empty() {
            return None;
        }
        let name = format!("{cut}{suffix}.{ext}");
        if !taken(&name) {
            return Some(name);
        }
    }
    None
}

/// Replaces what Windows forbids in a name with readable stand-ins and trims what it would strip.
fn clean(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            ':' | '|' | '/' | '\\' => out.push_str(" - "),
            '"' => out.push('\''),
            '<' | '>' | '?' | '*' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    trim_end(&collapsed).to_owned()
}

/// Windows drops trailing dots and spaces, and a zero-width joiner or variation selector left at
/// the end would belong to a character that was cut away.
fn trim_end(s: &str) -> &str {
    s.trim_end_matches(|c: char| {
        c == '.' || c.is_whitespace() || c == '\u{200d}' || ('\u{fe00}'..='\u{fe0f}').contains(&c)
    })
}

/// Where a reserved device word ends, when the part before the first dot is one. Windows ignores
/// spaces after the word, so `nul .txt` is the device too.
fn reserved_device(stem: &str) -> Option<usize> {
    let device = stem.split('.').next().unwrap_or(stem).trim_end();
    RESERVED
        .iter()
        .any(|r| device.eq_ignore_ascii_case(r))
        .then_some(device.len())
}

/// The longest prefix of `stem` within `units` UTF-16 units, never splitting a character.
fn truncate(stem: &str, units: usize) -> &str {
    let mut used = 0;
    let mut end = 0;
    for (i, c) in stem.char_indices() {
        used += c.len_utf16();
        if used > units {
            break;
        }
        end = i + c.len_utf8();
    }
    trim_end(&stem[..end])
}
