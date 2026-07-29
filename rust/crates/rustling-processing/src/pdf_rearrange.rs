use std::{collections::HashSet, path::Path};

use lopdf::{Document, Object};
use thiserror::Error;

use crate::{
    page_selection::{PageSelectionError, parse_page_list},
    pdf_page_geometry::materialize_inherited_attributes,
};

#[derive(Debug, Error)]
pub enum RearrangePagesError {
    #[error(transparent)]
    PageSelection(#[from] PageSelectionError),
    #[error("unsupported custom mode: {0}")]
    UnsupportedMode(String),
    #[error("duplicateCount must not exceed {maximum}")]
    DuplicateLimit { maximum: usize },
    #[error("could not read '{filename}' as a PDF: {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("could not rearrange PDF pages: {0}")]
    Update(#[from] lopdf::Error),
    #[error("the rearranged page count exceeds the PDF integer range")]
    PageCount,
    #[error("could not write the rearranged PDF: {0}")]
    Write(#[from] std::io::Error),
}

/// Rearranges pages using the custom order and predefined modes exposed by Java.
///
/// # Errors
///
/// Returns an error for unsafe selection expressions, unsupported modes,
/// excessive duplication, malformed PDFs, or output write failures.
pub fn rearrange_pdf_pages_to_file(
    input_path: &Path,
    filename: &str,
    page_numbers: Option<&str>,
    custom_mode: Option<&str>,
    output_path: &Path,
) -> Result<(), RearrangePagesError> {
    let mut document =
        Document::load(input_path).map_err(|source| RearrangePagesError::ReadPdf {
            filename: filename.to_owned(),
            source,
        })?;
    let page_ids: Vec<_> = document.get_pages().into_values().collect();
    let page_order = page_order(
        custom_mode,
        page_numbers.unwrap_or_default(),
        page_ids.len(),
    )?;
    let root_pages_id = document.catalog()?.get(b"Pages")?.as_reference()?;

    // Every page below is re-parented onto the root `/Pages` node, which cuts
    // it off from any intermediate node it used to inherit `/MediaBox`,
    // `/CropBox`, `/Rotate` or `/Resources` from. Pin those down first, while
    // the original tree is still intact.
    for &page_id in &page_ids {
        materialize_inherited_attributes(&mut document, page_id)?;
    }

    let mut seen = HashSet::new();
    let mut new_page_ids = Vec::with_capacity(page_order.len());
    for index in page_order {
        let source_id = page_ids[index];
        let page_id = if seen.insert(index) {
            source_id
        } else {
            let cloned_page = document.get_dictionary(source_id)?.clone();
            document.add_object(cloned_page)
        };
        document
            .get_dictionary_mut(page_id)?
            .set("Parent", root_pages_id);
        new_page_ids.push(Object::Reference(page_id));
    }

    let count = i64::try_from(new_page_ids.len()).map_err(|_| RearrangePagesError::PageCount)?;
    let root_pages = document.get_dictionary_mut(root_pages_id)?;
    root_pages.set("Kids", new_page_ids);
    root_pages.set("Count", count);
    document.save(output_path)?;
    Ok(())
}

fn page_order(
    custom_mode: Option<&str>,
    page_numbers: &str,
    total_pages: usize,
) -> Result<Vec<usize>, RearrangePagesError> {
    let Some(mode) = custom_mode.filter(|mode| !mode.is_empty()) else {
        return Ok(parse_page_list(page_numbers, total_pages)?);
    };
    if mode.eq_ignore_ascii_case("custom") {
        return Ok(parse_page_list(page_numbers, total_pages)?);
    }

    match mode.to_ascii_uppercase().as_str() {
        "REVERSE_ORDER" => Ok((0..total_pages).rev().collect()),
        "DUPLEX_SORT" => Ok(duplex_sort(total_pages)),
        "BOOKLET_SORT" => Ok(booklet_sort(total_pages)),
        "SIDE_STITCH_BOOKLET_SORT" => Ok(side_stitch_booklet_sort(total_pages)),
        "ODD_EVEN_SPLIT" => Ok(odd_even_split(total_pages)),
        "REMOVE_FIRST" => Ok((1..total_pages).collect()),
        "REMOVE_LAST" => Ok((0..total_pages.saturating_sub(1)).collect()),
        "REMOVE_FIRST_AND_LAST" => Ok(if total_pages <= 2 {
            Vec::new()
        } else {
            (1..total_pages - 1).collect()
        }),
        "DUPLICATE" => duplicate_order(total_pages, page_numbers),
        _ => Err(RearrangePagesError::UnsupportedMode(mode.to_owned())),
    }
}

fn duplex_sort(total_pages: usize) -> Vec<usize> {
    let half = total_pages.div_ceil(2);
    let mut result = Vec::with_capacity(total_pages);
    for page in 1..=half {
        result.push(page - 1);
        if page <= total_pages - half {
            result.push(total_pages - page);
        }
    }
    result
}

fn booklet_sort(total_pages: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(total_pages - total_pages % 2);
    for page in 0..total_pages / 2 {
        result.push(page);
        result.push(total_pages - page - 1);
    }
    result
}

fn side_stitch_booklet_sort(total_pages: usize) -> Vec<usize> {
    if total_pages == 0 {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(total_pages.div_ceil(4) * 4);
    for group in 0..total_pages.div_ceil(4) {
        let begin = group * 4;
        result.push((begin + 3).min(total_pages - 1));
        result.push(begin.min(total_pages - 1));
        result.push((begin + 1).min(total_pages - 1));
        result.push((begin + 2).min(total_pages - 1));
    }
    result
}

fn odd_even_split(total_pages: usize) -> Vec<usize> {
    (0..total_pages)
        .step_by(2)
        .chain((1..total_pages).step_by(2))
        .collect()
}

fn duplicate_order(
    total_pages: usize,
    page_numbers: &str,
) -> Result<Vec<usize>, RearrangePagesError> {
    let mut duplicate_count = page_numbers.trim().parse::<usize>().unwrap_or(2);
    if duplicate_count < 1 {
        duplicate_count = 2;
    }
    let maximum = 100usize.max(total_pages.saturating_mul(3));
    if duplicate_count > maximum {
        return Err(RearrangePagesError::DuplicateLimit { maximum });
    }
    let capacity = total_pages
        .checked_mul(duplicate_count)
        .ok_or(RearrangePagesError::PageCount)?;
    let mut result = Vec::with_capacity(capacity);
    for page in 0..total_pages {
        result.extend(std::iter::repeat_n(page, duplicate_count));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use lopdf::{Document, Object, Stream, dictionary};

    use super::{page_order, rearrange_pdf_pages_to_file};

    /// Two pages under an intermediate `/Pages` node that supplies `/MediaBox`
    /// and `/Rotate`, which is legal (ISO 32000-1 7.7.3.4) and which
    /// re-rooting the tree silently destroys.
    fn nested_page_tree_pdf() -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let root_pages_id = document.new_object_id();
        let intermediate_id = document.new_object_id();
        let page_ids = ["one", "two"]
            .into_iter()
            .map(|label| {
                let content = document.add_object(Stream::new(
                    dictionary! {},
                    format!("BT /F1 24 Tf 72 700 Td ({label}) Tj ET\n").into_bytes(),
                ));
                document.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => intermediate_id,
                    "Resources" => dictionary! {},
                    "Contents" => content,
                })
            })
            .collect::<Vec<_>>();
        document.objects.insert(
            intermediate_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Rotate" => 90,
                "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => 2,
            }),
        );
        document.objects.insert(
            root_pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(intermediate_id)],
                "Count" => 2,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => root_pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        let _ = document.save_to(&mut bytes);
        bytes
    }

    /// The defect: every mode re-parents pages onto the root `/Pages` node, so
    /// attributes held by an intermediate node vanished. The output had no
    /// `/MediaBox` and no `/Rotate`, and readers fell back to Letter.
    #[test]
    fn rearranging_keeps_attributes_inherited_from_intermediate_nodes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("nested.pdf");
        std::fs::write(&input, nested_page_tree_pdf())?;

        for mode in ["REVERSE_ORDER", "DUPLICATE", "ODD_EVEN_SPLIT"] {
            let output = directory.path().join(format!("{mode}.pdf"));
            rearrange_pdf_pages_to_file(&input, "nested.pdf", Some("2"), Some(mode), &output)?;
            let document = Document::load(&output)?;
            let pages = document.get_pages();
            assert!(!pages.is_empty(), "{mode} produced no pages");
            for page_id in pages.into_values() {
                let page = document.get_dictionary(page_id)?;
                let media_box = page
                    .get(b"MediaBox")
                    .map_err(|_| format!("{mode}: page lost its MediaBox"))?
                    .as_array()?
                    .iter()
                    .map(Object::as_float)
                    .collect::<Result<Vec<_>, _>>()?;
                assert_eq!(media_box, vec![0.0, 0.0, 595.0, 842.0], "{mode}");
                assert_eq!(
                    page.get(b"Rotate")
                        .map_err(|_| format!("{mode}: page lost its Rotate"))?
                        .as_i64()?,
                    90,
                    "{mode}"
                );
                assert!(page.has(b"Resources"), "{mode}: page lost its Resources");
            }
        }
        Ok(())
    }

    /// Upstream's DUPLICATE mode once put the same page node under `/Kids`
    /// several times, which makes the page tree cyclic (Stirling-PDF #6851).
    /// Every slot must stay a distinct object.
    #[test]
    fn duplicate_mode_emits_a_distinct_page_object_per_slot()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("nested.pdf");
        let output = directory.path().join("duplicated.pdf");
        std::fs::write(&input, nested_page_tree_pdf())?;
        rearrange_pdf_pages_to_file(&input, "nested.pdf", Some("4"), Some("DUPLICATE"), &output)?;

        let document = Document::load(&output)?;
        let root_pages_id = document.catalog()?.get(b"Pages")?.as_reference()?;
        let kids = document
            .get_dictionary(root_pages_id)?
            .get(b"Kids")?
            .as_array()?
            .iter()
            .map(Object::as_reference)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(kids.len(), 8);
        assert_eq!(
            kids.iter().collect::<HashSet<_>>().len(),
            8,
            "duplicated slots must not share one page object"
        );
        Ok(())
    }

    #[test]
    fn matches_the_java_predefined_orders() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(page_order(Some("REVERSE_ORDER"), "", 4)?, vec![3, 2, 1, 0]);
        assert_eq!(page_order(Some("DUPLEX_SORT"), "", 5)?, vec![0, 4, 1, 3, 2]);
        assert_eq!(page_order(Some("BOOKLET_SORT"), "", 5)?, vec![0, 4, 1, 3]);
        assert_eq!(
            page_order(Some("SIDE_STITCH_BOOKLET_SORT"), "", 6)?,
            vec![3, 0, 1, 2, 5, 4, 5, 5]
        );
        assert_eq!(
            page_order(Some("ODD_EVEN_SPLIT"), "", 6)?,
            vec![0, 2, 4, 1, 3, 5]
        );
        Ok(())
    }

    #[test]
    fn duplicates_each_page_with_distinct_slots() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            page_order(Some("DUPLICATE"), "3", 2)?,
            vec![0, 0, 0, 1, 1, 1]
        );
        Ok(())
    }
}
