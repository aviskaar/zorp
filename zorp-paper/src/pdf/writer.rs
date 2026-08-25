//! The PDF file container: indirect objects, the cross-reference table,
//! and the trailer. Nothing here knows what a paper is.

use std::fmt::Write as _;

#[derive(Default)]
pub struct Pdf {
    objects: Vec<Vec<u8>>,
}

impl Pdf {
    pub fn new() -> Pdf {
        Pdf::default()
    }

    /// Claim an object number. Bodies can be filled in later, which is
    /// what page objects need: they refer to their content stream and
    /// the page tree refers to them.
    pub fn reserve(&mut self) -> usize {
        self.objects.push(Vec::new());
        self.objects.len()
    }

    pub fn put(&mut self, id: usize, body: impl Into<Vec<u8>>) {
        self.objects[id - 1] = body.into();
    }

    /// Serialize a stream object: a dictionary carrying `/Length`,
    /// followed by the raw bytes. Uncompressed, deliberately. There is
    /// no flate implementation in this crate and adding a dependency for
    /// one would cost more than the bytes are worth, and an uncompressed
    /// content stream is something a test, or a person, can read.
    pub fn stream(data: &str) -> Vec<u8> {
        let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data.as_bytes());
        out.extend_from_slice(b"\nendstream");
        out
    }

    pub fn finish(self, root: usize, info: usize) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n");

        let mut offsets = Vec::with_capacity(self.objects.len());
        for (index, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }

        let xref_at = out.len();
        let count = self.objects.len() + 1;
        let mut xref = format!("xref\n0 {count}\n0000000000 65535 f \n");
        for offset in &offsets {
            let _ = writeln!(xref, "{offset:010} 00000 n ");
        }
        out.extend_from_slice(xref.as_bytes());
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {count} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
            )
            .as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_are_numbered_from_one() {
        let mut pdf = Pdf::new();
        assert_eq!(pdf.reserve(), 1);
        assert_eq!(pdf.reserve(), 2);
    }

    #[test]
    fn the_xref_offsets_land_on_their_objects() {
        let mut pdf = Pdf::new();
        let a = pdf.reserve();
        let b = pdf.reserve();
        pdf.put(a, "<< /Type /Catalog >>");
        pdf.put(b, "<< /Producer (t) >>");
        let bytes = pdf.finish(a, b);

        let text = String::from_utf8(bytes.clone()).unwrap();
        let xref_at: usize = text
            .rsplit_once("startxref\n")
            .unwrap()
            .1
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(bytes[xref_at..].starts_with(b"xref\n"));

        let table = &text[xref_at..];
        let mut lines = table.lines().skip(3);
        for id in 1..=2usize {
            let offset: usize = lines.next().unwrap()[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{id} 0 obj\n").as_bytes()));
        }
    }

    #[test]
    fn a_stream_declares_its_own_length() {
        let stream = Pdf::stream("BT ET");
        let text = String::from_utf8(stream).unwrap();
        assert!(text.starts_with("<< /Length 5 >>\nstream\n"), "{text}");
        assert!(text.ends_with("\nendstream"), "{text}");
    }

    #[test]
    fn the_trailer_names_the_catalog_and_the_info_dictionary() {
        let mut pdf = Pdf::new();
        let a = pdf.reserve();
        let b = pdf.reserve();
        pdf.put(a, "<< >>");
        pdf.put(b, "<< >>");
        let text = String::from_utf8(pdf.finish(a, b)).unwrap();
        assert!(text.contains("/Root 1 0 R"), "{text}");
        assert!(text.contains("/Info 2 0 R"), "{text}");
        assert!(text.contains("/Size 3"), "{text}");
    }
}
