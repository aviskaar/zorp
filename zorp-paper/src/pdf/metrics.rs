//! Character widths for the PDF base-14 fonts.
//!
//! A PDF viewer already has Times and Courier, so this crate embeds no
//! font data and the files it writes stay small. The cost is that
//! nothing else knows how wide a string is, and line breaking needs to.
//! These are Adobe's published Core-14 AFM widths, in 1/1000 em, for the
//! printable ASCII range.
//!
//! A wrong number here costs line-fill quality and nothing else: the
//! viewer places glyphs from its own copy of the metrics, so text never
//! overlaps or shifts because of a bad entry. Only the decision about
//! where to break a line would be slightly off.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Roman,
    Bold,
    Italic,
    Mono,
}

impl Face {
    /// The resource name this face is registered under on every page.
    pub fn resource(&self) -> &'static str {
        match self {
            Face::Roman => "F1",
            Face::Bold => "F2",
            Face::Italic => "F3",
            Face::Mono => "F4",
        }
    }

    pub fn base_font(&self) -> &'static str {
        match self {
            Face::Roman => "Times-Roman",
            Face::Bold => "Times-Bold",
            Face::Italic => "Times-Italic",
            Face::Mono => "Courier",
        }
    }
}

pub const ALL_FACES: [Face; 4] = [Face::Roman, Face::Bold, Face::Italic, Face::Mono];

/// Widths for codes 32..=126, in order.
#[rustfmt::skip]
const TIMES_ROMAN: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444,
    921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722,
    556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500,
    333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500,
    500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

#[rustfmt::skip]
const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500,
    930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778,
    611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, 333, 278, 333, 581, 500,
    333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, 556, 500,
    556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

#[rustfmt::skip]
const TIMES_ITALIC: [u16; 95] = [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250, 278,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 675, 675, 675, 500,
    920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556, 833, 667, 722,
    611, 722, 611, 500, 556, 722, 611, 833, 611, 556, 556, 389, 278, 389, 422, 500,
    333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278, 444, 278, 722, 500, 500,
    500, 500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,
];

/// The width of one character, in 1/1000 em.
///
/// Anything outside printable ASCII falls back to the width of `n`. That
/// covers the WinAnsi punctuation this crate maps into (curly quotes,
/// dashes) at a small cost in break quality, and the substitution
/// character everything else becomes.
pub fn width(face: Face, c: char) -> u16 {
    if face == Face::Mono {
        // Courier is monospaced. Every glyph, including the ones this
        // crate substitutes, is 600.
        return 600;
    }
    let table = match face {
        Face::Bold => &TIMES_BOLD,
        Face::Italic => &TIMES_ITALIC,
        _ => &TIMES_ROMAN,
    };
    let code = c as u32;
    match code {
        0x20..=0x7E => table[(code - 0x20) as usize],
        // Wide punctuation worth getting right, because a paragraph full
        // of them measured as `n` breaks visibly short. These are the
        // published widths for the same three fonts.
        0x2014 | 0x2026 => 1000,
        0x2013 => 500,
        _ => table[('n' as u32 - 0x20) as usize],
    }
}

/// The width of a string at `size` points.
pub fn text_width(face: Face, text: &str, size: f32) -> f32 {
    let total: u32 = text.chars().map(|c| u32::from(width(face, c))).sum();
    total as f32 * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_covers_printable_ascii() {
        assert_eq!(TIMES_ROMAN.len(), 95);
        assert_eq!(TIMES_BOLD.len(), 95);
        assert_eq!(TIMES_ITALIC.len(), 95);
    }

    #[test]
    fn a_space_is_narrower_than_a_capital_m() {
        for face in [Face::Roman, Face::Bold, Face::Italic] {
            assert!(width(face, ' ') < width(face, 'M'), "{face:?}");
        }
    }

    #[test]
    fn courier_is_monospaced() {
        assert_eq!(width(Face::Mono, 'i'), width(Face::Mono, 'W'));
    }

    #[test]
    fn bold_is_wider_than_roman_where_it_should_be() {
        assert!(width(Face::Bold, 'n') > width(Face::Roman, 'n'));
    }

    #[test]
    fn an_unmapped_character_still_has_a_width() {
        assert_eq!(width(Face::Roman, '\u{4e2d}'), width(Face::Roman, 'n'));
    }

    #[test]
    fn text_width_scales_with_size() {
        let small = text_width(Face::Roman, "hello", 10.0);
        let large = text_width(Face::Roman, "hello", 20.0);
        assert!((large - small * 2.0).abs() < 0.001);
    }

    #[test]
    fn the_empty_string_is_zero_wide() {
        assert_eq!(text_width(Face::Roman, "", 12.0), 0.0);
    }

    #[test]
    fn each_face_has_its_own_resource_name_and_base_font() {
        let mut names: Vec<&str> = ALL_FACES.iter().map(|f| f.resource()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL_FACES.len());
        assert_eq!(Face::Roman.base_font(), "Times-Roman");
        assert_eq!(Face::Mono.base_font(), "Courier");
    }
}
