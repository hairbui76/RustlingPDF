use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use thiserror::Error;

use crate::pdf_page_geometry::inherited_value;

#[derive(Debug, Clone)]
pub struct OverlayInput {
    pub filename: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct OverlayOptions {
    pub mode: String,
    pub counts: Vec<i32>,
    pub foreground: bool,
}

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("base PDF '{filename}' has no pages")]
    EmptyBase { filename: String },
    #[error("at least one overlay PDF is required")]
    MissingOverlay,
    #[error("overlay PDF '{filename}' has no pages")]
    EmptyOverlay { filename: String },
    #[error("invalid overlay mode '{0}'")]
    InvalidMode(String),
    #[error("counts array length must match the number of overlay files")]
    CountsLengthMismatch,
    #[error("PDF structure error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not write overlaid PDF: {0}")]
    WritePdf(std::io::Error),
}

#[derive(Debug, Clone, Copy)]
struct OverlayForm {
    id: ObjectId,
    width: f32,
    height: f32,
    rotation: i32,
}

/// Applies the requested Java-compatible overlay guide and writes a new PDF.
///
/// # Errors
///
/// Returns [`OverlayError`] when an input is invalid, its PDF structure cannot
/// be imported, or the output cannot be written.
pub fn overlay_pdf_paths_to_file(
    base_path: &Path,
    base_filename: &str,
    overlays: &[OverlayInput],
    options: &OverlayOptions,
    output_path: &Path,
) -> Result<(), OverlayError> {
    if overlays.is_empty() {
        return Err(OverlayError::MissingOverlay);
    }

    let mut document = load_document(base_path, base_filename)?;
    document.renumber_objects_with(1);
    let base_pages = document.get_pages().into_values().collect::<Vec<_>>();
    if base_pages.is_empty() {
        return Err(OverlayError::EmptyBase {
            filename: base_filename.to_owned(),
        });
    }

    let mut imported_forms = Vec::with_capacity(overlays.len());
    let mut page_counts = Vec::with_capacity(overlays.len());
    let mut next_object_id = document.max_id.saturating_add(1);
    for overlay in overlays {
        let mut source = load_document(&overlay.path, &overlay.filename)?;
        source.renumber_objects_with(next_object_id);
        let page_ids = source.get_pages().into_values().collect::<Vec<_>>();
        if page_ids.is_empty() {
            return Err(OverlayError::EmptyOverlay {
                filename: overlay.filename.clone(),
            });
        }
        let forms = page_ids
            .into_iter()
            .map(|page_id| overlay_form(&mut source, page_id))
            .collect::<Result<Vec<_>, _>>()?;
        next_object_id = source.max_id.saturating_add(1);
        page_counts.push(forms.len());
        imported_forms.push(forms);
        document.objects.extend(source.objects);
        document.max_id = document.max_id.max(source.max_id);
    }

    let guide = prepare_overlay_guide(base_pages.len(), &page_counts, options)?;
    for (page_index, selected) in guide.into_iter().enumerate() {
        let Some((overlay_index, overlay_page_index)) = selected else {
            continue;
        };
        let form = imported_forms[overlay_index][overlay_page_index];
        apply_overlay(
            &mut document,
            base_pages[page_index],
            form,
            page_index,
            options.foreground,
        )?;
    }

    document.renumber_objects();
    document.compress();
    document.save(output_path).map_err(OverlayError::WritePdf)?;
    Ok(())
}

fn load_document(path: &Path, filename: &str) -> Result<Document, OverlayError> {
    Document::load(path).map_err(|source| OverlayError::ReadPdf {
        filename: filename.to_owned(),
        source,
    })
}

fn prepare_overlay_guide(
    base_page_count: usize,
    page_counts: &[usize],
    options: &OverlayOptions,
) -> Result<Vec<Option<(usize, usize)>>, OverlayError> {
    match options.mode.as_str() {
        "SequentialOverlay" => Ok(sequential_guide(base_page_count, page_counts)),
        "InterleavedOverlay" => Ok(interleaved_guide(base_page_count, page_counts.len())),
        "FixedRepeatOverlay" => fixed_repeat_guide(base_page_count, page_counts, &options.counts),
        _ => Err(OverlayError::InvalidMode(options.mode.clone())),
    }
}

fn sequential_guide(base_page_count: usize, page_counts: &[usize]) -> Vec<Option<(usize, usize)>> {
    let mut guide = Vec::with_capacity(base_page_count);
    let mut overlay_file_index = 0usize;
    let mut page_in_overlay = 0usize;
    for _ in 0..base_page_count {
        if page_in_overlay == 0 || page_in_overlay >= page_counts[overlay_file_index] {
            page_in_overlay = 0;
            overlay_file_index = (overlay_file_index + 1) % page_counts.len();
        }
        guide.push(Some((overlay_file_index, page_in_overlay)));
        page_in_overlay += 1;
    }
    guide
}

fn interleaved_guide(base_page_count: usize, overlay_count: usize) -> Vec<Option<(usize, usize)>> {
    (0..base_page_count)
        .map(|page_index| Some((page_index % overlay_count, 0)))
        .collect()
}

fn fixed_repeat_guide(
    base_page_count: usize,
    page_counts: &[usize],
    counts: &[i32],
) -> Result<Vec<Option<(usize, usize)>>, OverlayError> {
    if page_counts.len() != counts.len() {
        return Err(OverlayError::CountsLengthMismatch);
    }
    let mut guide = vec![None; base_page_count];
    let mut current_page = 0usize;
    for (overlay_index, (&overlay_page_count, &repeat_count)) in
        page_counts.iter().zip(counts).enumerate()
    {
        for _ in 0..repeat_count.max(0) {
            for _ in 0..overlay_page_count {
                if current_page >= base_page_count {
                    return Ok(guide);
                }
                // PDFBox's specific-file overlay path constructs a layout from page zero.
                guide[current_page] = Some((overlay_index, 0));
                current_page += 1;
            }
        }
    }
    Ok(guide)
}

fn overlay_form(document: &mut Document, page_id: ObjectId) -> Result<OverlayForm, lopdf::Error> {
    let media_box = inherited_value(document, page_id, b"MediaBox")?;
    let (_, media_box) = document.dereference(&media_box)?;
    let media_box = media_box.as_array()?;
    let lower_x = media_box[0].as_float()?;
    let lower_y = media_box[1].as_float()?;
    let upper_x = media_box[2].as_float()?;
    let upper_y = media_box[3].as_float()?;
    let width = upper_x - lower_x;
    let height = upper_y - lower_y;
    let rotation = if let Ok(value) = inherited_value(document, page_id, b"Rotate") {
        document
            .dereference(&value)
            .ok()
            .and_then(|(_, value)| value.as_i64().ok())
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(0)
            .rem_euclid(360)
    } else {
        0
    };
    let resources = inherited_value(document, page_id, b"Resources")
        .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
    let content = document.get_page_content(page_id);
    let matrix = match rotation {
        90 => vec![
            0.into(),
            (-1).into(),
            1.into(),
            0.into(),
            0.into(),
            width.into(),
        ],
        180 => vec![
            (-1).into(),
            0.into(),
            0.into(),
            (-1).into(),
            width.into(),
            height.into(),
        ],
        270 => vec![
            0.into(),
            1.into(),
            (-1).into(),
            0.into(),
            height.into(),
            0.into(),
        ],
        _ => vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()],
    };
    let id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), width.into(), height.into()],
            "Matrix" => matrix,
            "Resources" => resources,
        },
        content,
    ));
    Ok(OverlayForm {
        id,
        width,
        height,
        rotation,
    })
}

fn apply_overlay(
    document: &mut Document,
    page_id: ObjectId,
    form: OverlayForm,
    page_index: usize,
    foreground: bool,
) -> Result<(), lopdf::Error> {
    let media_box = inherited_value(document, page_id, b"MediaBox")?;
    let (_, media_box) = document.dereference(&media_box)?;
    let media_box = media_box.as_array()?;
    let base_width = media_box[2].as_float()? - media_box[0].as_float()?;
    let base_height = media_box[3].as_float()? - media_box[1].as_float()?;
    let (overlay_width, overlay_height) = if matches!(form.rotation, 90 | 270) {
        (form.height, form.width)
    } else {
        (form.width, form.height)
    };
    let translate_x = (base_width - overlay_width) / 2.0;
    let translate_y = (base_height - overlay_height) / 2.0;
    let resource_name = format!("OL{page_index}");
    install_xobject(document, page_id, resource_name.as_bytes(), form.id)?;

    let overlay_content =
        format!("q\nq\n1 0 0 1 {translate_x} {translate_y} cm\n/{resource_name} Do Q\nQ\n");
    let overlay_id = document.add_object(Stream::new(dictionary! {}, overlay_content.into_bytes()));
    let original = document
        .get_dictionary(page_id)?
        .get(b"Contents")
        .ok()
        .cloned();
    let mut contents = Vec::new();
    if foreground {
        let save_id = document.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
        let restore_id = document.add_object(Stream::new(dictionary! {}, b"Q\n".to_vec()));
        contents.push(Object::Reference(save_id));
        append_original_contents(document, original, &mut contents);
        contents.push(Object::Reference(restore_id));
        contents.push(Object::Reference(overlay_id));
    } else {
        contents.push(Object::Reference(overlay_id));
        append_original_contents(document, original, &mut contents);
    }
    document
        .get_dictionary_mut(page_id)?
        .set("Contents", contents);
    Ok(())
}

pub(crate) fn install_xobject(
    document: &mut Document,
    page_id: ObjectId,
    name: &[u8],
    form_id: ObjectId,
) -> Result<(), lopdf::Error> {
    let resources = inherited_value(document, page_id, b"Resources")
        .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
    let (_, resources) = document.dereference(&resources)?;
    let mut resources = resources.as_dict()?.clone();
    let mut xobjects = match resources.get(b"XObject") {
        Ok(value) => {
            let (_, value) = document.dereference(value)?;
            value.as_dict()?.clone()
        }
        Err(_) => Dictionary::new(),
    };
    xobjects.set(name, form_id);
    resources.set("XObject", xobjects);
    document
        .get_dictionary_mut(page_id)?
        .set("Resources", resources);
    Ok(())
}

/// Appends a content stream to a page the way `PDFBox`'s
/// `PDPageContentStream(doc, page, AppendMode.APPEND, true, true)` does.
///
/// The final `true` is `PDFBox`'s `resetContext`: it inserts a stream holding a
/// bare `q` at the front of the page's `/Contents` array and opens the appended
/// stream with the matching `Q`. Because the array is consumed as one
/// concatenated stream, that `Q` pops whatever the page's own content left on
/// the graphics state stack. Without it a page whose content ends inside
/// `q ... cm` silently drags that transform onto everything we draw afterwards,
/// which is exactly how stamps, overlaid images and watermarks end up in the
/// wrong place at the wrong size.
///
/// For balanced content the wrapper is a visual no-op: `q` pushes the initial
/// state and `Q` pops it right back off.
///
/// The wrapper is emitted only when the page already has content, mirroring
/// `PDFBox`'s `sourcePage.hasContents()` guard - a lone `Q` on an empty page
/// would underflow the graphics state stack.
///
/// # What the single `Q` does and does not restore
///
/// The appended content resumes from the state captured by the most recent `q`
/// the page's own stream left unmatched, or from the page's initial state when
/// that stream is balanced. The count of unmatched `q`s is irrelevant:
/// `q q 0.5 0 0 0.5 300 400 cm` resets cleanly, because the innermost unmatched
/// `q` saved the state from before the `cm` ran.
///
/// What survives is whatever state was already in force when that most recent
/// unmatched `q` ran, whatever nesting depth it sits at. A page whose content
/// opens `0.5 0 0 0.5 0 0 cm` and only afterwards leaves a `q` unmatched still
/// scales the appended content by 0.5, because the `Q` restores the state as of
/// that `q` - which already carried the scale. Depth is not what decides it:
/// `q 2 0 0 2 0 0 cm q 0.5 0 0 0.5 0 0 cm` leaves the depth-1 doubling in
/// force, while `q 9 0 0 9 0 0 cm Q` followed by `0.5 0 0 0.5 0 0 cm` resets
/// cleanly even though the scale was set at depth zero. A stream with excess
/// `Q` is a third shape: the surplus `Q` consumes the prefix stream's own save,
/// so the appended `Q` underflows and later state leaks.
/// `PDFBox` behaves identically, writing the same single `q`/`Q` pair, so this
/// is a shared residual limit rather than a divergence.
pub(crate) fn append_page_content_with_reset(
    document: &mut Document,
    page_id: ObjectId,
    content: &[u8],
) -> Result<(), lopdf::Error> {
    let original = document
        .get_dictionary(page_id)?
        .get(b"Contents")
        .ok()
        .cloned();
    let reset_context = page_has_contents(document, original.as_ref());

    let mut appended = Vec::with_capacity(content.len() + 3);
    if reset_context {
        // The leading newline keeps the `Q` a token of its own even when the
        // preceding stream ends mid-token; consumers are required to insert a
        // separator between array members, but not all of them do.
        appended.extend_from_slice(b"\nQ\n");
    }
    appended.extend_from_slice(content);
    let appended_id = document.add_object(Stream::new(dictionary! {}, appended));

    let mut contents = Vec::new();
    if reset_context {
        let save_id = document.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
        contents.push(Object::Reference(save_id));
    }
    append_original_contents(document, original, &mut contents);
    contents.push(Object::Reference(appended_id));
    document
        .get_dictionary_mut(page_id)?
        .set("Contents", contents);
    Ok(())
}

/// Mirrors `PDFBox` `PDPage.hasContents()`: content exists when `/Contents`
/// resolves to a non-empty stream or a non-empty array.
fn page_has_contents(document: &Document, contents: Option<&Object>) -> bool {
    let Some(contents) = contents else {
        return false;
    };
    match document.dereference(contents) {
        Ok((_, Object::Stream(stream))) => !stream.content.is_empty(),
        Ok((_, Object::Array(items))) => !items.is_empty(),
        _ => false,
    }
}

fn append_original_contents(
    document: &mut Document,
    contents: Option<Object>,
    output: &mut Vec<Object>,
) {
    match contents {
        None | Some(Object::Null) => {}
        Some(Object::Array(items)) => output.extend(items),
        Some(Object::Reference(id)) => output.push(Object::Reference(id)),
        Some(Object::Stream(stream)) => {
            let id = document.add_object(stream);
            output.push(Object::Reference(id));
        }
        Some(other) => output.push(other),
    }
}

/// Helpers shared by the graphics-state regression tests of every surface that
/// appends content to an existing page.
#[cfg(test)]
pub(crate) mod test_support {
    use lopdf::{Document, Object, ObjectId, Stream, content::Content, dictionary};

    /// A content stream that leaves a `q` open after scaling and translating,
    /// exactly the shape that used to drag its transform onto appended content.
    pub(crate) const UNBALANCED_CONTENT: &[u8] =
        b"q 0.5 0 0 0.5 300 400 cm\nBT /F1 36 Tf 60 700 Td (UNBALANCED-Q) Tj ET\n";

    /// The same drawing with the `q` properly closed.
    pub(crate) const BALANCED_CONTENT: &[u8] =
        b"q 0.5 0 0 0.5 300 400 cm\nBT /F1 36 Tf 60 700 Td (BALANCED-Q) Tj ET\nQ\n";

    /// Two unmatched `q`s, both opened before anything touched the state. The
    /// single `Q` still restores the initial state, so nesting depth alone
    /// never defeats the reset.
    pub(crate) const DOUBLY_UNBALANCED_CONTENT: &[u8] =
        b"q q 0.5 0 0 0.5 300 400 cm\nBT /F1 36 Tf 60 700 Td (DOUBLE-Q) Tj ET\n";

    /// The shape the reset genuinely cannot repair: the transform is applied at
    /// nesting depth zero, *before* the stream leaves a `q` unmatched, so the
    /// state the `Q` restores already carries it.
    pub(crate) const LEAK_OUTSIDE_SAVE_CONTENT: &[u8] =
        b"0.5 0 0 0.5 0 0 cm\nq 1 0 0 1 100 100 cm\nBT /F1 36 Tf 60 700 Td (OUTER-CM) Tj ET\n";

    /// Builds a single-page A4 PDF whose page content is exactly `content`.
    pub(crate) fn single_page_pdf(content: &[u8]) -> Vec<u8> {
        page_pdf(Some(content), [0, 0, 595, 842])
    }

    /// Builds a single-page PDF with the requested `/MediaBox`, and with no
    /// `/Contents` entry at all when `content` is `None`.
    pub(crate) fn page_pdf(content: Option<&[u8]>, media_box: [i64; 4]) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let tree_root_id = document.new_object_id();
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => tree_root_id,
            "MediaBox" => media_box.into_iter().map(Object::Integer).collect::<Vec<_>>(),
            "Resources" => dictionary! {},
        };
        if let Some(content) = content {
            let content_id = document.add_object(Stream::new(dictionary! {}, content.to_vec()));
            page.set("Contents", content_id);
        }
        let page_id = document.add_object(page);
        document.objects.insert(
            tree_root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => tree_root_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        let _ = document.save_to(&mut bytes);
        bytes
    }

    /// Replays a page's concatenated content stream and returns the current
    /// transformation matrix in force when `name` is drawn with `Do`.
    ///
    /// This is the matrix a viewer actually applies to the appended artwork, so
    /// comparing it against the same operation on balanced input proves where
    /// the mark lands without needing a rasterizer.
    pub(crate) fn ctm_at_xobject(
        document: &Document,
        page_id: ObjectId,
        name: &str,
    ) -> Option<[f32; 6]> {
        let content = Content::decode(&document.get_page_content(page_id)).ok()?;
        let mut ctm = IDENTITY;
        let mut stack: Vec<[f32; 6]> = Vec::new();
        for operation in &content.operations {
            match operation.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(ctm),
                "cm" => {
                    let operands = operation
                        .operands
                        .iter()
                        .map(Object::as_float)
                        .collect::<Result<Vec<_>, _>>()
                        .ok()?;
                    ctm = concat(<[f32; 6]>::try_from(operands).ok()?, ctm);
                }
                "Do" => {
                    let drawn = operation
                        .operands
                        .first()
                        .and_then(|operand| operand.as_name().ok());
                    if drawn == Some(name.as_bytes()) {
                        return Some(ctm);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    /// `left` applied before `right`, which is what `cm` does to the CTM.
    fn concat(left: [f32; 6], right: [f32; 6]) -> [f32; 6] {
        [
            left[0] * right[0] + left[1] * right[2],
            left[0] * right[1] + left[1] * right[3],
            left[2] * right[0] + left[3] * right[2],
            left[2] * right[1] + left[3] * right[3],
            left[4] * right[0] + left[5] * right[2] + right[4],
            left[4] * right[1] + left[5] * right[3] + right[5],
        ]
    }

    /// Compares two matrices with a tolerance that absorbs the five-decimal
    /// rounding our content writers apply.
    pub(crate) fn assert_matrix_eq(actual: [f32; 6], expected: [f32; 6]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.001,
                "matrix mismatch: {actual:?} vs {expected:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use lopdf::{Document, Object, ObjectId, Stream, dictionary};

    use super::{
        OverlayOptions, append_page_content_with_reset, prepare_overlay_guide,
        test_support::{
            BALANCED_CONTENT, DOUBLY_UNBALANCED_CONTENT, IDENTITY, LEAK_OUTSIDE_SAVE_CONTENT,
            UNBALANCED_CONTENT, assert_matrix_eq, ctm_at_xobject, page_pdf, single_page_pdf,
        },
    };

    fn first_page(document: &Document) -> Result<ObjectId, Box<dyn std::error::Error>> {
        Ok(*document
            .get_pages()
            .values()
            .next()
            .ok_or("the fixture has no page")?)
    }

    /// The defect: content appended after an unbalanced `q ... cm` inherited
    /// that transform, so a mark asked for in unscaled page space was drawn
    /// half-size and 300x400 away from where it belonged.
    #[test]
    fn appended_content_starts_from_the_initial_graphics_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::load_mem(&single_page_pdf(UNBALANCED_CONTENT))?;
        let page_id = first_page(&document)?;
        append_page_content_with_reset(&mut document, page_id, b"q /Mark Do Q\n")?;
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            IDENTITY,
        );
        Ok(())
    }

    /// Wrapping already-balanced content in `q`/`Q` has to change nothing.
    #[test]
    fn balanced_pages_are_unaffected_by_the_reset() -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::load_mem(&single_page_pdf(BALANCED_CONTENT))?;
        let page_id = first_page(&document)?;
        append_page_content_with_reset(&mut document, page_id, b"q /Mark Do Q\n")?;
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            IDENTITY,
        );
        Ok(())
    }

    /// The reset is not a count of `q`s: two saves opened before the transform
    /// ran still reset cleanly, because the innermost unmatched `q` captured
    /// the state from before the `cm`.
    #[test]
    fn several_unmatched_saves_still_reset_to_the_initial_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::load_mem(&single_page_pdf(DOUBLY_UNBALANCED_CONTENT))?;
        let page_id = first_page(&document)?;
        append_page_content_with_reset(&mut document, page_id, b"q /Mark Do Q\n")?;
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            IDENTITY,
        );
        Ok(())
    }

    /// Pins the residual limit the single `q`/`Q` pair cannot cover, so it is
    /// learned from a test rather than from prose.
    ///
    /// The page scales at nesting depth zero and only then leaves a `q`
    /// unmatched, so the state that `q` saved already carries the scale and the
    /// `Q` restores it. The appended content keeps the 0.5 scale but does lose
    /// the inner `100 100` translate. `PDFBox` produces the same result; this
    /// documents shared behaviour, not a regression.
    #[test]
    fn state_changed_outside_any_save_survives_the_reset() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut document = Document::load_mem(&single_page_pdf(LEAK_OUTSIDE_SAVE_CONTENT))?;
        let page_id = first_page(&document)?;
        append_page_content_with_reset(&mut document, page_id, b"q /Mark Do Q\n")?;
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            [0.5, 0.0, 0.0, 0.5, 0.0, 0.0],
        );
        Ok(())
    }

    /// A page with no `/Contents` must not get a `Q` with nothing to restore -
    /// that underflows the graphics state stack. `PDFBox` guards this the same
    /// way with `sourcePage.hasContents()`.
    #[test]
    fn pages_without_content_get_no_unbalanced_restore() -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::load_mem(&page_pdf(None, [0, 0, 595, 842]))?;
        let page_id = first_page(&document)?;
        // A payload with no `Q` of its own, so any restore in the result can
        // only have come from the reset wrapper.
        append_page_content_with_reset(&mut document, page_id, b"/Mark Do\n")?;
        let content = String::from_utf8_lossy(&document.get_page_content(page_id)).into_owned();
        assert!(
            !content.contains('Q'),
            "an empty page must not gain a restore operator: {content}"
        );
        assert_eq!(
            document
                .get_dictionary(page_id)?
                .get(b"Contents")?
                .as_array()?
                .len(),
            1,
            "an empty page must not gain a prefix save stream either"
        );
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            IDENTITY,
        );
        Ok(())
    }

    /// The same has to hold when `/Contents` is already an array rather than a
    /// single stream: every original member survives, in order, between the
    /// new save and restore.
    #[test]
    fn existing_contents_arrays_keep_every_member() -> Result<(), Box<dyn std::error::Error>> {
        let mut document = Document::load_mem(&single_page_pdf(b"q 2 0 0 2 0 0 cm\n"))?;
        let page_id = first_page(&document)?;
        let second = document.add_object(Stream::new(dictionary! {}, b"1 0 0 1 7 9 cm\n".to_vec()));
        let first = document.get_dictionary(page_id)?.get(b"Contents")?.clone();
        document
            .get_dictionary_mut(page_id)?
            .set("Contents", vec![first, Object::Reference(second)]);

        append_page_content_with_reset(&mut document, page_id, b"q /Mark Do Q\n")?;
        let content = String::from_utf8_lossy(&document.get_page_content(page_id)).into_owned();
        assert!(content.contains("2 0 0 2 0 0 cm"), "{content}");
        assert!(content.contains("1 0 0 1 7 9 cm"), "{content}");
        assert_matrix_eq(
            ctm_at_xobject(&document, page_id, "Mark").ok_or("the mark is not drawn")?,
            IDENTITY,
        );
        Ok(())
    }

    #[test]
    fn sequential_mode_preserves_java_file_switch_order() -> Result<(), super::OverlayError> {
        let guide = prepare_overlay_guide(
            7,
            &[2, 3],
            &OverlayOptions {
                mode: "SequentialOverlay".to_owned(),
                counts: Vec::new(),
                foreground: true,
            },
        )?;
        assert_eq!(
            guide,
            vec![
                Some((1, 0)),
                Some((1, 1)),
                Some((1, 2)),
                Some((0, 0)),
                Some((0, 1)),
                Some((1, 0)),
                Some((1, 1)),
            ]
        );
        Ok(())
    }

    #[test]
    fn fixed_repeat_uses_first_page_and_leaves_the_remainder_unchanged()
    -> Result<(), super::OverlayError> {
        let guide = prepare_overlay_guide(
            7,
            &[2, 3],
            &OverlayOptions {
                mode: "FixedRepeatOverlay".to_owned(),
                counts: vec![1, 1],
                foreground: false,
            },
        )?;
        assert_eq!(
            guide,
            vec![
                Some((0, 0)),
                Some((0, 0)),
                Some((1, 0)),
                Some((1, 0)),
                Some((1, 0)),
                None,
                None,
            ]
        );
        Ok(())
    }
}
