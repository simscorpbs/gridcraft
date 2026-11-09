//! One custom document property in a finished package (`docProps/custom.xml`, ECMA-376 Part 1
//! §22.3), read or set without opening the workbook.
//!
//! GridCraft's version history keeps a random document id there. Excel keeps custom properties
//! when it saves (they are the Custom tab of Advanced Properties), so the id survives a round
//! trip through Excel, and a file that is renamed, moved or copied by other programs carries it.
//!
//! [`set_custom_property`] copies every other part of the package byte for byte (their
//! compressed bytes are not touched); only `docProps/custom.xml`, and the content-types and
//! package-relationship entries when they lack the part, are written.

use std::io::{Cursor, Read, Write};

use crate::IoError;

/// Relationship type of the custom-properties part.
const REL_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const DEFAULT_PART: &str = "docProps/custom.xml";
/// Most bytes read from one of the small XML parts looked at here.
const MAX_XML: u64 = 1 << 20;
/// Format id Office gives every custom property.
const FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";
const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

fn read_part<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut part = zip.by_name(name).ok()?;
    if part.size() > MAX_XML {
        return None;
    }
    let mut text = String::new();
    part.by_ref().take(MAX_XML).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The part name the package's relationships give the custom properties, else the usual one.
fn custom_part_name(rels: &str) -> String {
    let mut rest = rels;
    while let Some(start) = rest.find("<Relationship ") {
        let Some(after) = rest.get(start..) else { break };
        let end = after.find('>').map_or(after.len(), |e| e + 1);
        let tag = after.get(..end).unwrap_or("");
        if attribute(tag, "Type").as_deref() == Some(REL_TYPE)
            && let Some(target) = attribute(tag, "Target")
            && !target.contains("://")
        {
            let name = target.trim_start_matches('/');
            if !name.is_empty() && !name.contains("..") {
                return name.to_owned();
            }
        }
        rest = after.get(end..).unwrap_or("");
    }
    DEFAULT_PART.to_owned()
}

/// The value of attribute `name` in an element's opening tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let key = format!("{name}={quote}");
        let mut from = 0;
        while let Some(found) = tag.get(from..).and_then(|t| t.find(&key)) {
            let at = from + found;
            let boundary = at == 0 || tag.as_bytes().get(at - 1).is_some_and(|b| b.is_ascii_whitespace());
            if boundary {
                let value = tag.get(at + key.len()..)?;
                return value.find(quote).and_then(|e| value.get(..e)).map(unescape);
            }
            from = at + key.len();
        }
    }
    None
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The text value of the property `name` in a `custom.xml` document, when it is a string.
fn property_value(xml: &str, name: &str) -> Option<String> {
    let (start, end) = property_span(xml, name)?;
    let property = xml.get(start..end)?;
    let open = property.find("lpwstr>").or_else(|| property.find("lpstr>"))?;
    let text = property.get(open..)?;
    let value_start = text.find('>')? + 1;
    let value = text.get(value_start..)?.split('<').next()?;
    Some(unescape(value.trim()))
}

/// Where the `<property>` named `name` starts and ends (after `</property>`).
fn property_span(xml: &str, name: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(found) = xml.get(from..).and_then(|t| t.find("<property")) {
        let start = from + found;
        let tag_end = start + xml.get(start..)?.find('>')? + 1;
        let tag = xml.get(start..tag_end)?;
        if tag.starts_with("<property ") || tag.starts_with("<property\t") || tag.starts_with("<property\n") {
            let end = if tag.ends_with("/>") { tag_end } else { tag_end + xml.get(tag_end..)?.find("</property>")? + "</property>".len() };
            if attribute(tag, "name").as_deref() == Some(name) {
                return Some((start, end));
            }
            from = end;
        } else {
            from = tag_end;
        }
    }
    None
}

/// The next free property id (they start at 2).
fn next_pid(xml: &str) -> u32 {
    let mut pid: u32 = 1;
    let mut from = 0;
    while let Some(found) = xml.get(from..).and_then(|t| t.find("pid=\"")) {
        let at = from + found + 5;
        let digits: String = xml.get(at..).unwrap_or("").chars().take_while(char::is_ascii_digit).collect();
        pid = pid.max(digits.parse().unwrap_or(1));
        from = at;
    }
    pid.saturating_add(1)
}

fn property_xml(pid: u32, name: &str, value: &str) -> String {
    format!("<property fmtid=\"{FMTID}\" pid=\"{pid}\" name=\"{}\"><vt:lpwstr>{}</vt:lpwstr></property>", escape(name), escape(value))
}

/// `custom.xml` with the property `name` set to `value`: replaced where it is, else added.
fn with_property(existing: Option<&str>, name: &str, value: &str) -> String {
    let Some(xml) = existing else {
        return format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"{NS}\" xmlns:vt=\"{NS_VT}\">{}</Properties>",
            property_xml(2, name, value)
        );
    };
    if let Some((start, end)) = property_span(xml, name) {
        let pid = xml.get(start..end).and_then(|p| attribute(p, "pid")).and_then(|p| p.parse().ok()).unwrap_or_else(|| next_pid(xml));
        return format!("{}{}{}", xml.get(..start).unwrap_or(""), property_xml(pid, name, value), xml.get(end..).unwrap_or(""));
    }
    let property = property_xml(next_pid(xml), name, value);
    if let Some(close) = xml.rfind("</Properties>") {
        return format!("{}{property}{}", xml.get(..close).unwrap_or(""), xml.get(close..).unwrap_or(""));
    }
    // An empty root element: `<Properties .../>`.
    if let Some(end) = xml.rfind("/>") {
        let head = xml.get(..end).unwrap_or("").trim_end();
        let prefixed = if head.contains("xmlns:vt=") { String::new() } else { format!(" xmlns:vt=\"{NS_VT}\"") };
        return format!("{head}{prefixed}>{property}</Properties>");
    }
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"{NS}\" xmlns:vt=\"{NS_VT}\">{property}</Properties>")
}

/// The string value of the custom property `name` of the package in `bytes`, or `None` when the
/// package, the part or the property is missing or unreadable.
pub fn custom_property(bytes: &[u8], name: &str) -> Option<String> {
    custom_property_in(Cursor::new(bytes), name)
}

/// [`custom_property`] for a package read from `reader` (a file): only the package index and the
/// two small parts looked at are read.
pub fn custom_property_in<R: Read + std::io::Seek>(reader: R, name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(reader).ok()?;
    let rels = read_part(&mut zip, "_rels/.rels").unwrap_or_default();
    let part = custom_part_name(&rels);
    property_value(&read_part(&mut zip, &part)?, name)
}

/// The package in `bytes` with the custom property `name` set to the string `value`. Every other
/// part is copied as it is; the property is added (with its content type and package
/// relationship, when the package lacks them) or replaced.
pub fn set_custom_property(bytes: &[u8], name: &str, value: &str) -> Result<Vec<u8>, IoError> {
    let zip_error = |e: zip::result::ZipError| IoError::Zip(e.to_string());
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let rels = read_part(&mut archive, "_rels/.rels");
    let types = read_part(&mut archive, "[Content_Types].xml");
    let part = custom_part_name(rels.as_deref().unwrap_or(""));
    let existing = read_part(&mut archive, &part);
    if existing.as_deref().and_then(|xml| property_value(xml, name)).as_deref() == Some(value) && rels.is_some() && types.is_some() {
        // Already there, and the part is declared: nothing to change.
        let declared = rels.as_deref().is_some_and(|r| r.contains(REL_TYPE)) && types.as_deref().is_some_and(|t| t.contains(&format!("/{part}")));
        if declared {
            return Ok(bytes.to_vec());
        }
    }
    let custom = with_property(existing.as_deref(), name, value);
    let types = types.map(|t| {
        if t.contains(&format!("PartName=\"/{part}\"")) {
            t
        } else {
            let entry = format!("<Override PartName=\"/{part}\" ContentType=\"{CONTENT_TYPE}\"/>");
            match t.rfind("</Types>") {
                Some(close) => format!("{}{entry}{}", t.get(..close).unwrap_or(""), t.get(close..).unwrap_or("")),
                None => t,
            }
        }
    });
    let rels = rels.map(|r| {
        if r.contains(REL_TYPE) {
            r
        } else {
            let id = (1..).map(|n| format!("rId{n}")).find(|id| !r.contains(&format!("Id=\"{id}\""))).unwrap_or_else(|| "rIdCustom".into());
            let entry = format!("<Relationship Id=\"{id}\" Type=\"{REL_TYPE}\" Target=\"{part}\"/>");
            match r.rfind("</Relationships>") {
                Some(close) => format!("{}{entry}{}", r.get(..close).unwrap_or(""), r.get(close..).unwrap_or("")),
                None => r,
            }
        }
    });

    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut wrote_custom = false;
    for index in 0..archive.len() {
        let entry_name = archive.by_index_raw(index).map_err(zip_error)?.name().to_owned();
        let replacement: Option<&[u8]> = if entry_name == part {
            wrote_custom = true;
            Some(custom.as_bytes())
        } else if entry_name == "[Content_Types].xml" {
            types.as_deref().map(str::as_bytes)
        } else if entry_name == "_rels/.rels" {
            rels.as_deref().map(str::as_bytes)
        } else {
            None
        };
        match replacement {
            Some(data) => {
                out.start_file(entry_name, options).map_err(zip_error)?;
                out.write_all(data).map_err(|e| IoError::Zip(e.to_string()))?;
            }
            None => out.raw_copy_file(archive.by_index_raw(index).map_err(zip_error)?).map_err(zip_error)?,
        }
    }
    if !wrote_custom {
        out.start_file(part, options).map_err(zip_error)?;
        out.write_all(custom.as_bytes()).map_err(|e| IoError::Zip(e.to_string()))?;
    }
    Ok(out.finish().map_err(zip_error)?.into_inner())
}

/// The package in `bytes` without the custom property `name`: `None` when it has no such
/// property (nothing to change). Every other part is copied as it is.
pub fn remove_custom_property(bytes: &[u8], name: &str) -> Result<Option<Vec<u8>>, IoError> {
    let zip_error = |e: zip::result::ZipError| IoError::Zip(e.to_string());
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let rels = read_part(&mut archive, "_rels/.rels").unwrap_or_default();
    let part = custom_part_name(&rels);
    let Some(xml) = read_part(&mut archive, &part) else { return Ok(None) };
    let Some((start, end)) = property_span(&xml, name) else { return Ok(None) };
    let custom = format!("{}{}", xml.get(..start).unwrap_or(""), xml.get(end..).unwrap_or(""));
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let entry_name = archive.by_index_raw(index).map_err(zip_error)?.name().to_owned();
        if entry_name == part {
            out.start_file(entry_name, options).map_err(zip_error)?;
            out.write_all(custom.as_bytes()).map_err(|e| IoError::Zip(e.to_string()))?;
        } else {
            out.raw_copy_file(archive.by_index_raw(index).map_err(zip_error)?).map_err(zip_error)?;
        }
    }
    Ok(Some(out.finish().map_err(zip_error)?.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "12345678-1234-4234-8234-1234567890ab";

    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in parts {
            w.start_file(*name, options).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    const TYPES: &str = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";

    #[test]
    fn a_package_without_custom_properties_gets_the_part_the_content_type_and_the_relationship() {
        let before = package(&[("[Content_Types].xml", TYPES), ("_rels/.rels", RELS), ("xl/workbook.xml", "<workbook/>")]);
        let after = set_custom_property(&before, "GridCraftDocumentId", ID).unwrap();
        assert_eq!(custom_property(&after, "GridCraftDocumentId").as_deref(), Some(ID));
        let mut zip = zip::ZipArchive::new(Cursor::new(after.as_slice())).unwrap();
        assert!(read_part(&mut zip, "[Content_Types].xml").unwrap().contains("/docProps/custom.xml"));
        let rels = read_part(&mut zip, "_rels/.rels").unwrap();
        assert!(rels.contains("custom-properties") && rels.contains("rId2"), "{rels}");
        assert_eq!(read_part(&mut zip, "xl/workbook.xml").as_deref(), Some("<workbook/>"));
    }

    #[test]
    fn other_properties_stay_and_the_id_is_replaced_in_place() {
        let custom = format!(
            "<Properties xmlns=\"{NS}\" xmlns:vt=\"{NS_VT}\"><property fmtid=\"{FMTID}\" pid=\"2\" name=\"Team\"><vt:lpwstr>A &amp; B</vt:lpwstr></property><property fmtid=\"{FMTID}\" pid=\"3\" name=\"GridCraftDocumentId\"><vt:lpwstr>old</vt:lpwstr></property></Properties>"
        );
        let before = package(&[("[Content_Types].xml", TYPES), ("_rels/.rels", RELS), ("docProps/custom.xml", &custom)]);
        let after = set_custom_property(&before, "GridCraftDocumentId", ID).unwrap();
        assert_eq!(custom_property(&after, "GridCraftDocumentId").as_deref(), Some(ID));
        assert_eq!(custom_property(&after, "Team").as_deref(), Some("A & B"));
        let again = set_custom_property(&after, "GridCraftDocumentId", ID).unwrap();
        assert_eq!(custom_property(&again, "GridCraftDocumentId").as_deref(), Some(ID));
        // A second property gets the next id.
        let two = set_custom_property(&after, "Other", "x").unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(two.as_slice())).unwrap();
        assert!(read_part(&mut zip, "docProps/custom.xml").unwrap().contains("pid=\"4\""));
    }

    #[test]
    fn a_property_can_be_removed_and_nothing_else_changes() {
        let before = package(&[("[Content_Types].xml", TYPES), ("_rels/.rels", RELS), ("xl/workbook.xml", "<workbook/>")]);
        assert!(remove_custom_property(&before, "GridCraftDocumentId").unwrap().is_none());
        let with_id = set_custom_property(&set_custom_property(&before, "Team", "A").unwrap(), "GridCraftDocumentId", ID).unwrap();
        let without = remove_custom_property(&with_id, "GridCraftDocumentId").unwrap().unwrap();
        assert_eq!(custom_property(&without, "GridCraftDocumentId"), None);
        assert_eq!(custom_property(&without, "Team").as_deref(), Some("A"));
    }

    #[test]
    fn a_damaged_package_is_an_error_and_a_missing_property_is_none() {
        assert!(set_custom_property(b"not a zip", "A", "b").is_err());
        assert_eq!(custom_property(b"not a zip", "A"), None);
        let plain = package(&[("[Content_Types].xml", TYPES)]);
        assert_eq!(custom_property(&plain, "A"), None);
    }
}
