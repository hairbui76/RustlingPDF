use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path},
};

use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::pdf_form_mutation::{FormMutationError, fill_fields_to_file};

const MAX_WORKBOOK_XML_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FormBatchError {
    #[error("could not read batch data '{filename}': {source}")]
    ReadData {
        filename: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid batch data: {0}")]
    InvalidData(String),
    #[error("could not read XLSX workbook: {0}")]
    Workbook(#[from] zip::result::ZipError),
    #[error(transparent)]
    Fill(#[from] FormMutationError),
    #[error("could not create batch archive: {0}")]
    Archive(std::io::Error),
    #[error("could not create batch archive: {0}")]
    ArchiveZip(zip::result::ZipError),
}

#[derive(Clone, Debug)]
struct BatchTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// Fills one PDF per nonblank CSV/XLSX row and packages the results in a ZIP.
///
/// The template and source data are never modified. Temporary row outputs are
/// scoped to this call and deleted when it returns.
///
/// # Errors
///
/// Returns [`FormBatchError`] when the source table is invalid, a row cannot
/// be filled, or the result archive cannot be written.
pub fn batch_fill_to_zip(
    pdf_path: &Path,
    pdf_filename: &str,
    data_path: &Path,
    data_filename: &str,
    output_path: &Path,
) -> Result<usize, FormBatchError> {
    let mut table = read_batch_table(data_path, data_filename)?;
    validate_headers(&mut table.headers)?;
    let temporary = tempfile::tempdir().map_err(FormBatchError::Archive)?;
    let filename_column = table
        .headers
        .iter()
        .position(|header| header == "_filename");
    let mut used_names = HashSet::new();
    let mut outputs = Vec::new();

    for row in &table.rows {
        if row.iter().all(|value| value.trim().is_empty()) {
            continue;
        }
        let requested_name = filename_column
            .and_then(|index| row.get(index))
            .map(String::as_str);
        let base_name = requested_name
            .and_then(sanitize_output_base)
            .unwrap_or_else(|| format!("row-{:03}", outputs.len() + 1));
        let output_name = unique_pdf_name(&base_name, &mut used_names);
        let values = table
            .headers
            .iter()
            .enumerate()
            .filter(|(_, header)| header.as_str() != "_filename")
            .map(|(index, header)| {
                (
                    header.clone(),
                    Some(row.get(index).cloned().unwrap_or_default()),
                )
            })
            .collect::<Vec<_>>();
        let row_path = temporary
            .path()
            .join(format!("row-{}.pdf", outputs.len() + 1));
        fill_fields_to_file(pdf_path, pdf_filename, &values, &row_path)?;
        outputs.push((output_name, row_path));
    }

    let output = File::create(output_path).map_err(FormBatchError::Archive)?;
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, path) in &outputs {
        archive
            .start_file(name, options)
            .map_err(FormBatchError::ArchiveZip)?;
        let bytes = fs::read(path).map_err(FormBatchError::Archive)?;
        archive.write_all(&bytes).map_err(FormBatchError::Archive)?;
    }
    archive.finish().map_err(FormBatchError::ArchiveZip)?;
    Ok(outputs.len())
}

fn read_batch_table(path: &Path, filename: &str) -> Result<BatchTable, FormBatchError> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => read_csv(path, filename),
        "xlsx" => read_xlsx(path),
        _ => Err(FormBatchError::InvalidData(
            "dataFile must be a .csv or .xlsx file".to_owned(),
        )),
    }
}

fn read_csv(path: &Path, filename: &str) -> Result<BatchTable, FormBatchError> {
    let file = File::open(path).map_err(|source| FormBatchError::ReadData {
        filename: filename.to_owned(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new().flexible(false).from_reader(file);
    let headers = reader
        .headers()
        .map_err(|error| FormBatchError::InvalidData(format!("invalid CSV header: {error}")))?
        .iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let rows = reader
        .records()
        .map(|row| {
            row.map(|row| row.iter().map(str::to_owned).collect::<Vec<_>>())
                .map_err(|error| FormBatchError::InvalidData(format!("invalid CSV row: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BatchTable { headers, rows })
}

fn read_xlsx(path: &Path) -> Result<BatchTable, FormBatchError> {
    let input = File::open(path).map_err(|source| FormBatchError::ReadData {
        filename: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workbook.xlsx")
            .to_owned(),
        source,
    })?;
    let mut archive = ZipArchive::new(input)?;
    let workbook = read_zip_text(&mut archive, "xl/workbook.xml")?;
    let relationships = read_zip_text(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let workbook_document = roxmltree::Document::parse(&workbook)
        .map_err(|error| FormBatchError::InvalidData(format!("invalid workbook XML: {error}")))?;
    let first_sheet = workbook_document
        .descendants()
        .find(|node| node.tag_name().name() == "sheet")
        .ok_or_else(|| FormBatchError::InvalidData("workbook has no worksheets".to_owned()))?;
    let relationship_id = first_sheet
        .attributes()
        .find(|attribute| attribute.name() == "id")
        .map(|attribute| attribute.value())
        .ok_or_else(|| {
            FormBatchError::InvalidData("first worksheet has no relationship id".to_owned())
        })?;
    let relationships_document = roxmltree::Document::parse(&relationships).map_err(|error| {
        FormBatchError::InvalidData(format!("invalid workbook relationships XML: {error}"))
    })?;
    let target = relationships_document
        .descendants()
        .find(|node| {
            node.tag_name().name() == "Relationship"
                && node.attribute("Id") == Some(relationship_id)
        })
        .and_then(|node| node.attribute("Target"))
        .ok_or_else(|| {
            FormBatchError::InvalidData("first worksheet relationship target is missing".to_owned())
        })?;
    let sheet_path = workbook_target_path(target)?;
    let sheet = read_zip_text(&mut archive, &sheet_path)?;
    let shared_strings = match archive.by_name("xl/sharedStrings.xml") {
        Ok(mut entry) => {
            if entry.size() > MAX_WORKBOOK_XML_BYTES {
                return Err(FormBatchError::InvalidData(
                    "shared strings XML exceeds the safety limit".to_owned(),
                ));
            }
            let mut xml = String::new();
            entry.read_to_string(&mut xml).map_err(|error| {
                FormBatchError::InvalidData(format!("could not read shared strings XML: {error}"))
            })?;
            parse_shared_strings(&xml)?
        }
        Err(zip::result::ZipError::FileNotFound) => Vec::new(),
        Err(error) => return Err(FormBatchError::Workbook(error)),
    };
    parse_sheet(&sheet, &shared_strings)
}

fn read_zip_text(archive: &mut ZipArchive<File>, name: &str) -> Result<String, FormBatchError> {
    let mut entry = archive.by_name(name)?;
    if entry.size() > MAX_WORKBOOK_XML_BYTES {
        return Err(FormBatchError::InvalidData(format!(
            "workbook part '{name}' exceeds the safety limit"
        )));
    }
    let mut text = String::new();
    entry.read_to_string(&mut text).map_err(|error| {
        FormBatchError::InvalidData(format!("could not read workbook part '{name}': {error}"))
    })?;
    Ok(text)
}

fn workbook_target_path(target: &str) -> Result<String, FormBatchError> {
    let target = target.trim_start_matches('/');
    let path = if target.starts_with("xl/") {
        target.to_owned()
    } else {
        format!("xl/{target}")
    };
    if Path::new(&path)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(FormBatchError::InvalidData(
            "first worksheet relationship target is unsafe".to_owned(),
        ));
    }
    Ok(path)
}

fn parse_shared_strings(xml: &str) -> Result<Vec<String>, FormBatchError> {
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        FormBatchError::InvalidData(format!("invalid shared strings XML: {error}"))
    })?;
    Ok(document
        .descendants()
        .filter(|node| node.tag_name().name() == "si")
        .map(descendant_text)
        .collect())
}

fn parse_sheet(xml: &str, shared_strings: &[String]) -> Result<BatchTable, FormBatchError> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| FormBatchError::InvalidData(format!("invalid worksheet XML: {error}")))?;
    let mut rows = Vec::new();
    for row in document
        .descendants()
        .filter(|node| node.tag_name().name() == "row")
    {
        let mut values = BTreeMap::new();
        let mut next_index = 0_usize;
        for cell in row.children().filter(|node| node.tag_name().name() == "c") {
            let index = cell
                .attribute("r")
                .and_then(cell_column_index)
                .unwrap_or(next_index);
            next_index = index.saturating_add(1);
            values.insert(index, cell_value(cell, shared_strings)?);
        }
        let width = values
            .last_key_value()
            .map_or(0, |(last_index, _)| last_index + 1);
        let mut ordered = vec![String::new(); width];
        for (index, value) in values {
            ordered[index] = value;
        }
        rows.push(ordered);
    }
    let headers = rows.first().cloned().unwrap_or_default();
    let data_width = headers.len();
    let rows = rows
        .into_iter()
        .skip(1)
        .map(|mut row| {
            row.resize(data_width, String::new());
            row.truncate(data_width);
            row
        })
        .collect();
    Ok(BatchTable { headers, rows })
}

fn cell_value(
    cell: roxmltree::Node<'_, '_>,
    shared_strings: &[String],
) -> Result<String, FormBatchError> {
    let cell_type = cell.attribute("t").unwrap_or_default();
    if cell_type == "inlineStr" {
        return Ok(descendant_text(cell));
    }
    let value = cell
        .children()
        .find(|node| node.tag_name().name() == "v")
        .and_then(|node| node.text())
        .unwrap_or_default();
    match cell_type {
        "s" => {
            let index = value.parse::<usize>().map_err(|error| {
                FormBatchError::InvalidData(format!(
                    "shared string index '{value}' is invalid: {error}"
                ))
            })?;
            shared_strings.get(index).cloned().ok_or_else(|| {
                FormBatchError::InvalidData(format!(
                    "shared string index {index} is outside the table"
                ))
            })
        }
        "b" => Ok(if value == "1" { "true" } else { "false" }.to_owned()),
        _ => Ok(value.to_owned()),
    }
}

fn descendant_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|descendant| descendant.tag_name().name() == "t")
        .filter_map(|descendant| descendant.text())
        .collect()
}

fn cell_column_index(reference: &str) -> Option<usize> {
    let mut value = 0_usize;
    let mut found = false;
    for byte in reference.bytes() {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        found = true;
        value = value
            .checked_mul(26)?
            .checked_add(usize::from(byte.to_ascii_uppercase() - b'A') + 1)?;
    }
    found.then(|| value - 1)
}

fn validate_headers(headers: &mut [String]) -> Result<(), FormBatchError> {
    if headers.is_empty() {
        return Err(FormBatchError::InvalidData(
            "the first row must contain at least one header".to_owned(),
        ));
    }
    let mut seen = HashSet::new();
    for header in headers {
        *header = header.trim().to_owned();
        if header.is_empty() {
            return Err(FormBatchError::InvalidData(
                "headers must not be blank".to_owned(),
            ));
        }
        if !seen.insert(header.clone()) {
            return Err(FormBatchError::InvalidData(format!(
                "duplicate header '{header}'"
            )));
        }
    }
    Ok(())
}

fn sanitize_output_base(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = value
        .strip_suffix(".pdf")
        .or_else(|| value.strip_suffix(".PDF"))
        .unwrap_or(value);
    let mut sanitized = String::new();
    let mut previous_separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_') {
            sanitized.push(character);
            previous_separator = false;
        } else if !previous_separator {
            sanitized.push('_');
            previous_separator = true;
        }
    }
    let sanitized = sanitized.trim_matches(['.', '_', ' ', '-']).to_owned();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn unique_pdf_name(base: &str, used_names: &mut HashSet<String>) -> String {
    let candidate = format!("{base}.pdf");
    if used_names.insert(candidate.clone()) {
        return candidate;
    }
    for index in 2_u64.. {
        let candidate = format!("{base}_{index}.pdf");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read as _, Write as _},
    };

    use lopdf::{Document, Object, dictionary};
    use tempfile::tempdir;
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

    use super::{
        batch_fill_to_zip, cell_column_index, parse_sheet, sanitize_output_base, unique_pdf_name,
    };

    #[test]
    fn parses_sparse_shared_and_inline_sheet_values() -> Result<(), Box<dyn std::error::Error>> {
        let sheet = r#"<worksheet><sheetData>
          <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="inlineStr"><is><t>flag</t></is></c></row>
          <row r="2"><c r="A2" t="s"><v>1</v></c><c r="C2" t="b"><v>1</v></c></row>
        </sheetData></worksheet>"#;
        let table = parse_sheet(sheet, &["name".to_owned(), "Alice".to_owned()])?;
        assert_eq!(table.headers, ["name", "", "flag"]);
        assert_eq!(table.rows, [vec!["Alice", "", "true"]]);
        assert_eq!(cell_column_index("AA12"), Some(26));
        Ok(())
    }

    #[test]
    fn sanitizes_and_deduplicates_output_names() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(
            sanitize_output_base("../Client report?.pdf").as_deref(),
            Some("Client_report")
        );
        assert_eq!(unique_pdf_name("Client", &mut used), "Client.pdf");
        assert_eq!(unique_pdf_name("Client", &mut used), "Client_2.pdf");
    }

    #[test]
    fn fills_quoted_csv_rows_and_packages_round_trip_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let pdf = temp.path().join("template.pdf");
        let data = temp.path().join("data.csv");
        let output = temp.path().join("output.zip");
        fs::write(&pdf, form_pdf()?)?;
        fs::write(
            &data,
            "name,_filename\r\n\"Ada, Lovelace\",\"../Report?.pdf\"\r\nGrace,Report\r\n",
        )?;
        assert_eq!(
            batch_fill_to_zip(&pdf, "template.pdf", &data, "data.csv", &output)?,
            2
        );
        let mut archive = ZipArchive::new(Cursor::new(fs::read(output)?))?;
        assert_eq!(archive.by_index(0)?.name(), "Report.pdf");
        assert_eq!(archive.by_index(1)?.name(), "Report_2.pdf");
        let mut first = Vec::new();
        archive.by_name("Report.pdf")?.read_to_end(&mut first)?;
        let document = Document::load_mem(&first)?;
        let acroform_id = document.catalog()?.get(b"AcroForm")?.as_reference()?;
        let field_id = document
            .get_dictionary(acroform_id)?
            .get(b"Fields")?
            .as_array()?[0]
            .as_reference()?;
        assert_eq!(
            lopdf::decode_text_string(document.get_dictionary(field_id)?.get(b"V")?)?,
            "Ada, Lovelace"
        );
        Ok(())
    }

    #[test]
    fn reads_first_xlsx_sheet_shared_and_inline_strings() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempdir()?;
        let pdf = temp.path().join("template.pdf");
        let data = temp.path().join("data.xlsx");
        let output = temp.path().join("output.zip");
        fs::write(&pdf, form_pdf()?)?;
        let file = fs::File::create(&data)?;
        let mut workbook = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        workbook.start_file("xl/workbook.xml", options)?;
        workbook.write_all(
            br#"<workbook xmlns:r="r"><sheets><sheet name="First" r:id="rId1"/></sheets></workbook>"#,
        )?;
        workbook.start_file("xl/_rels/workbook.xml.rels", options)?;
        workbook.write_all(
            br#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#,
        )?;
        workbook.start_file("xl/sharedStrings.xml", options)?;
        workbook.write_all(br"<sst><si><t>name</t></si><si><t>Lin</t></si></sst>")?;
        workbook.start_file("xl/worksheets/sheet1.xml", options)?;
        workbook.write_all(
            br#"<worksheet><sheetData>
              <row><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>_filename</t></is></c></row>
              <row><c r="A2" t="s"><v>1</v></c><c r="B2" t="inlineStr"><is><t>Person</t></is></c></row>
            </sheetData></worksheet>"#,
        )?;
        workbook.finish()?;
        assert_eq!(
            batch_fill_to_zip(&pdf, "template.pdf", &data, "data.xlsx", &output)?,
            1
        );
        let mut archive = ZipArchive::new(Cursor::new(fs::read(output)?))?;
        assert_eq!(archive.by_index(0)?.name(), "Person.pdf");
        Ok(())
    }

    fn form_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.7");
        let page_tree_id = document.new_object_id();
        let leaf_page_id = document.new_object_id();
        let field_id = document.new_object_id();
        let widget_id = document.new_object_id();
        document.objects.insert(
            widget_id,
            Object::Dictionary(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Parent" => field_id,
                "P" => leaf_page_id,
                "Rect" => vec![10.into(), 10.into(), 110.into(), 30.into()],
            }),
        );
        document.objects.insert(
            field_id,
            Object::Dictionary(dictionary! {
                "FT" => "Tx",
                "T" => Object::string_literal("name"),
                "Kids" => vec![Object::Reference(widget_id)],
                "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
            }),
        );
        document.objects.insert(
            leaf_page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => page_tree_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
                "Annots" => vec![Object::Reference(widget_id)],
            }),
        );
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(leaf_page_id)],
                "Count" => 1,
            }),
        );
        let acroform_id = document.add_object(dictionary! {
            "Fields" => vec![Object::Reference(field_id)],
            "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => page_tree_id,
            "AcroForm" => acroform_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;
        Ok(bytes)
    }
}
