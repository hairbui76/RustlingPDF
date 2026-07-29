use std::{collections::HashSet, path::Path};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content, dictionary};
use regex::Regex;
use thiserror::Error;

use crate::{pdf_page_geometry::inherited_value, pdf_signatures::flatten_signature_fields};

#[derive(Debug, Error)]
pub enum DocumentOperationError {
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("PDF structure error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not prepare the XFA read-only matcher: {0}")]
    Regex(#[from] regex::Error),
    #[error("could not write PDF: {0}")]
    Write(std::io::Error),
}

/// Flattens root signature fields and writes an unsigned PDF.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF cannot be read, transformed,
/// or written.
pub fn remove_cert_sign_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, |document| {
        flatten_signature_fields(document)?;
        Ok(())
    })
}

/// Decodes every supported stream and saves without recompression.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF cannot be read or written.
pub fn decompress_pdf_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, |document| {
        document.decompress();
        Ok(())
    })
}

/// Clears read-only form flags, field locks, and XFA `readOnly` access markers.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF or its form tree cannot be
/// processed or written.
pub fn unlock_pdf_forms_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, unlock_forms)
}

/// Removes image `XObjects` from page and nested Form `XObject` resources.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF resource tree cannot be read,
/// transformed, or written.
pub fn remove_images_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, remove_images)
}

/// Strips every raster image from a PDF: image `XObject` resources and the `Do`
/// operators that paint them, inline `BI`/`ID`/`EI` images, and finally the now
/// unreferenced image streams themselves.
///
/// This is the pure-Rust replacement for the Ghostscript `-dFILTERIMAGE` pass the
/// OCR endpoint used for `removeImagesAfter=true`. Unlike
/// [`remove_images_to_file`], which only detaches images from the resource tree,
/// this also deletes the drawing operators and prunes the orphaned streams, so the
/// image bytes are gone from the saved file rather than merely unreferenced.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF cannot be read, its content
/// streams cannot be decoded or re-encoded, or the result cannot be written.
pub fn strip_images_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, |document| {
        strip_images(document)?;
        document.prune_objects();
        Ok(())
    })
}

fn strip_images(document: &mut Document) -> Result<(), DocumentOperationError> {
    let image_ids = image_object_ids(document);
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    let mut visited_forms = HashSet::new();
    for page_id in page_ids {
        let resources = inherited_value(document, page_id, b"Resources")
            .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
        let (resources, removed) = detach_image_xobjects(document, &resources, &image_ids)?;
        let mut form_ids = Vec::new();
        collect_form_xobject_ids(document, &resources, &mut form_ids);
        document
            .get_dictionary_mut(page_id)?
            .set("Resources", resources);
        let content = document.get_page_content(page_id);
        let rewritten = drop_image_operations(&content, &removed)?;
        document.change_page_content(page_id, rewritten)?;
        for form_id in form_ids {
            strip_images_from_form(document, form_id, &image_ids, &mut visited_forms)?;
        }
    }
    Ok(())
}

fn strip_images_from_form(
    document: &mut Document,
    form_id: ObjectId,
    image_ids: &HashSet<ObjectId>,
    visited_forms: &mut HashSet<ObjectId>,
) -> Result<(), DocumentOperationError> {
    if !visited_forms.insert(form_id) {
        return Ok(());
    }
    let form = document.get_object(form_id)?.as_stream()?.clone();
    let resources = form
        .dict
        .get(b"Resources")
        .cloned()
        .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
    let (resources, removed) = detach_image_xobjects(document, &resources, image_ids)?;
    let mut child_ids = Vec::new();
    collect_form_xobject_ids(document, &resources, &mut child_ids);
    let content = form.decompressed_content()?;
    let rewritten = drop_image_operations(&content, &removed)?;
    {
        let stream = document.get_object_mut(form_id)?.as_stream_mut()?;
        stream.dict.set("Resources", resources);
        stream.set_plain_content(rewritten);
    }
    for child_id in child_ids {
        strip_images_from_form(document, child_id, image_ids, visited_forms)?;
    }
    Ok(())
}

fn image_object_ids(document: &Document) -> HashSet<ObjectId> {
    document
        .objects
        .iter()
        .filter(|(_, object)| {
            object.as_stream().is_ok_and(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|subtype| subtype == b"Image")
            })
        })
        .map(|(object_id, _)| *object_id)
        .collect()
}

/// Removes image entries from an `XObject` resource dictionary and reports the
/// resource names that were removed, so the matching `Do` operators can go too.
fn detach_image_xobjects(
    document: &mut Document,
    resources: &Object,
    image_ids: &HashSet<ObjectId>,
) -> Result<(Object, HashSet<Vec<u8>>), DocumentOperationError> {
    let (_, resolved) = document.dereference(resources)?;
    let mut dictionary = resolved.as_dict()?.clone();
    let mut removed = HashSet::new();
    let Ok(xobjects) = dictionary.get(b"XObject").cloned() else {
        return Ok((Object::Dictionary(dictionary), removed));
    };
    let (_, xobjects) = document.dereference(&xobjects)?;
    let mut xobjects = xobjects.as_dict()?.clone();
    let names = xobjects
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    for name in names {
        let is_image = xobjects.get(&name).is_ok_and(|xobject| match xobject {
            Object::Reference(object_id) => image_ids.contains(object_id),
            Object::Stream(stream) => stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|subtype| subtype == b"Image"),
            _ => false,
        });
        if is_image {
            xobjects.remove(&name);
            removed.insert(name);
        }
    }
    // The rewritten XObject dictionary is inlined into the resources so an
    // indirect dictionary shared with an untouched page is never mutated in place.
    dictionary.set("XObject", Object::Dictionary(xobjects));
    Ok((Object::Dictionary(dictionary), removed))
}

fn collect_form_xobject_ids(document: &Document, resources: &Object, forms: &mut Vec<ObjectId>) {
    let Some(dictionary) = resources.as_dict().ok() else {
        return;
    };
    let Ok(Object::Dictionary(xobjects)) = dictionary.get(b"XObject") else {
        return;
    };
    for (_, xobject) in xobjects {
        let Ok(object_id) = xobject.as_reference() else {
            continue;
        };
        let is_form = document
            .get_object(object_id)
            .and_then(Object::as_stream)
            .is_ok_and(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|subtype| subtype == b"Form")
            });
        if is_form && !forms.contains(&object_id) {
            forms.push(object_id);
        }
    }
}

/// Drops `Do` operators naming a removed image resource and every inline image.
fn drop_image_operations(
    content: &[u8],
    removed: &HashSet<Vec<u8>>,
) -> Result<Vec<u8>, DocumentOperationError> {
    let mut decoded = Content::decode(content)?;
    decoded.operations.retain(|operation| {
        match operation.operator.as_str() {
            // `BI` carries the whole inline image (dictionary plus samples) as its
            // operand, so dropping the operation removes the pixels as well.
            "BI" => false,
            "Do" => !operation
                .operands
                .first()
                .and_then(|operand| operand.as_name().ok())
                .is_some_and(|name| removed.contains(name)),
            _ => true,
        }
    });
    Ok(decoded.encode()?)
}

/// Performs the Java controller's dependency-free repair fallback by parsing
/// and rewriting the PDF structure.
///
/// # Errors
///
/// Returns [`DocumentOperationError`] when the PDF cannot be parsed or the
/// normalized output cannot be written.
pub fn repair_pdf_to_file(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), DocumentOperationError> {
    transform_pdf(input_path, filename, output_path, |_| Ok(()))
}

fn transform_pdf(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
    transform: impl FnOnce(&mut Document) -> Result<(), DocumentOperationError>,
) -> Result<(), DocumentOperationError> {
    let mut document =
        Document::load(input_path).map_err(|source| DocumentOperationError::ReadPdf {
            filename: filename.to_owned(),
            source,
        })?;
    transform(&mut document)?;
    document
        .save(output_path)
        .map_err(DocumentOperationError::Write)?;
    Ok(())
}

fn unlock_forms(document: &mut Document) -> Result<(), DocumentOperationError> {
    let Ok(acroform) = document.catalog()?.get(b"AcroForm").cloned() else {
        return Ok(());
    };
    let (acroform_id, acroform_object) = document.dereference(&acroform)?;
    let mut acroform_dictionary = acroform_object.as_dict()?.clone();
    acroform_dictionary.set("NeedAppearances", true);
    if let Ok(fields) = acroform_dictionary.get(b"Fields").cloned() {
        let (_, fields) = document.dereference(&fields)?;
        let fields = fields.as_array()?.clone();
        let fields = fields
            .into_iter()
            .map(|field| unlock_field(document, field, 0, &mut HashSet::new()))
            .collect::<Result<Vec<_>, _>>()?;
        acroform_dictionary.set("Fields", fields);
    }
    if let Ok(xfa) = acroform_dictionary.get(b"XFA").cloned() {
        acroform_dictionary.set("XFA", unlock_xfa(document, &xfa)?);
    }
    write_dictionary(document, acroform_id, b"AcroForm", acroform_dictionary)?;
    Ok(())
}

fn unlock_field(
    document: &mut Document,
    field: Object,
    inherited_flags: i64,
    visited: &mut HashSet<ObjectId>,
) -> Result<Object, DocumentOperationError> {
    let (field_id, resolved) = document.dereference(&field)?;
    if let Some(field_id) = field_id
        && !visited.insert(field_id)
    {
        return Ok(field);
    }
    let mut dictionary = resolved.as_dict()?.clone();
    dictionary.remove(b"Lock");
    let current_flags = dictionary
        .get(b"Ff")
        .ok()
        .and_then(|flags| document.dereference(flags).ok())
        .and_then(|(_, flags)| flags.as_i64().ok())
        .unwrap_or(inherited_flags);
    if current_flags & 1 == 1 || dictionary.has(b"Ff") {
        dictionary.set("Ff", current_flags & !1);
    }
    if let Ok(kids) = dictionary.get(b"Kids").cloned() {
        let (_, kids) = document.dereference(&kids)?;
        let kids = kids.as_array()?.clone();
        let kids = kids
            .into_iter()
            .map(|kid| unlock_field(document, kid, current_flags & !1, visited))
            .collect::<Result<Vec<_>, _>>()?;
        dictionary.set("Kids", kids);
    }
    if let Some(field_id) = field_id {
        document
            .objects
            .insert(field_id, Object::Dictionary(dictionary));
        Ok(Object::Reference(field_id))
    } else {
        Ok(Object::Dictionary(dictionary))
    }
}

fn unlock_xfa(document: &mut Document, xfa: &Object) -> Result<Object, DocumentOperationError> {
    let read_only = Regex::new(r#"access\s*=\s*"readOnly""#)?;
    let (_, resolved) = document.dereference(xfa)?;
    let resolved = resolved.clone();
    match resolved {
        Object::Stream(stream) => replacement_xfa_stream(document, &stream, &read_only),
        Object::Array(mut parts) => {
            for index in (1..parts.len()).step_by(2) {
                let (_, stream) = document.dereference(&parts[index])?;
                if let Object::Stream(stream) = stream {
                    let stream = stream.clone();
                    parts[index] = replacement_xfa_stream(document, &stream, &read_only)?;
                }
            }
            Ok(Object::Array(parts))
        }
        _ => Ok(xfa.clone()),
    }
}

fn replacement_xfa_stream(
    document: &mut Document,
    stream: &Stream,
    read_only: &Regex,
) -> Result<Object, DocumentOperationError> {
    let xml = stream.decompressed_content()?;
    let xml = String::from_utf8_lossy(&xml);
    let opened = read_only.replace_all(&xml, "access=\"open\"");
    let id = document.add_object(Stream::new(dictionary! {}, opened.as_bytes().to_vec()));
    Ok(Object::Reference(id))
}

fn write_dictionary(
    document: &mut Document,
    object_id: Option<ObjectId>,
    catalog_key: &[u8],
    dictionary: Dictionary,
) -> Result<(), lopdf::Error> {
    if let Some(object_id) = object_id {
        document
            .objects
            .insert(object_id, Object::Dictionary(dictionary));
    } else {
        document
            .catalog_mut()?
            .set(catalog_key, Object::Dictionary(dictionary));
    }
    Ok(())
}

fn remove_images(document: &mut Document) -> Result<(), DocumentOperationError> {
    let page_ids = document.get_pages().into_values().collect::<Vec<_>>();
    let mut visited_forms = HashSet::new();
    for page_id in page_ids {
        let resources = inherited_value(document, page_id, b"Resources")
            .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
        let resources = clean_resources(document, &resources, &mut visited_forms)?;
        document
            .get_dictionary_mut(page_id)?
            .set("Resources", resources);
    }
    Ok(())
}

fn clean_resources(
    document: &mut Document,
    resources: &Object,
    visited_forms: &mut HashSet<ObjectId>,
) -> Result<Object, DocumentOperationError> {
    let (resources_id, resolved) = document.dereference(resources)?;
    let mut dictionary = resolved.as_dict()?.clone();
    if let Ok(xobjects) = dictionary.get(b"XObject").cloned() {
        let (_, xobjects) = document.dereference(&xobjects)?;
        let mut xobjects = xobjects.as_dict()?.clone();
        let names = xobjects
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        for name in names {
            let Some(xobject) = xobjects.get(&name).ok().cloned() else {
                continue;
            };
            let (object_id, resolved) = document.dereference(&xobject)?;
            let Object::Stream(stream) = resolved else {
                continue;
            };
            match stream.dict.get(b"Subtype").and_then(Object::as_name) {
                Ok(b"Image") => {
                    xobjects.remove(&name);
                }
                Ok(b"Form") => {
                    if object_id.is_some_and(|id| !visited_forms.insert(id)) {
                        continue;
                    }
                    let mut stream = stream.clone();
                    if let Ok(form_resources) = stream.dict.get(b"Resources").cloned() {
                        let cleaned = clean_resources(document, &form_resources, visited_forms)?;
                        stream.dict.set("Resources", cleaned);
                        if let Some(object_id) = object_id {
                            document.objects.insert(object_id, Object::Stream(stream));
                        } else {
                            xobjects.set(name.clone(), Object::Stream(stream));
                        }
                    }
                }
                _ => {}
            }
        }
        dictionary.set("XObject", xobjects);
    }
    if let Some(resources_id) = resources_id {
        document
            .objects
            .insert(resources_id, Object::Dictionary(dictionary));
        Ok(Object::Reference(resources_id))
    } else {
        Ok(Object::Dictionary(dictionary))
    }
}

#[cfg(test)]
mod strip_images_tests {
    use lopdf::{Document, Object, Stream, content::Content, dictionary};

    use super::strip_images_to_file;

    /// The OCR `removeImagesAfter` replacement must delete the image resource,
    /// the `Do` that paints it, the inline image, and the image bytes, while
    /// leaving the text-showing operators and their font resource untouched.
    #[test]
    fn strips_image_xobjects_inline_images_and_their_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");

        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
                "BitsPerComponent" => 8,
            },
            b"IMAGEPIXELBYTES".to_vec(),
        ));
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 10 10 Td (OCRTEXTLAYER) Tj ET\n\
              q 40 0 0 40 5 5 cm /Im0 Do Q\n\
              BI /W 1 /H 1 /CS /G /BPC 8 ID \x11 EI"
                .to_vec(),
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font_id },
                "XObject" => dictionary! { "Im0" => Object::Reference(image_id) },
            },
            "Contents" => content_id,
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.save(&input)?;

        strip_images_to_file(&input, "input.pdf", &output)?;

        let bytes = std::fs::read(&output)?;
        assert!(
            !bytes
                .windows(b"IMAGEPIXELBYTES".len())
                .any(|window| window == b"IMAGEPIXELBYTES"),
            "the image stream survived stripping"
        );

        let stripped = Document::load(&output)?;
        let page_id = stripped
            .get_pages()
            .into_values()
            .next()
            .ok_or("no page after stripping")?;
        let content = Content::decode(&stripped.get_page_content(page_id))?;
        let operators = content
            .operations
            .iter()
            .map(|operation| operation.operator.as_str())
            .collect::<Vec<_>>();
        assert!(!operators.contains(&"Do"), "{operators:?}");
        assert!(!operators.contains(&"BI"), "{operators:?}");
        assert!(operators.contains(&"Tj"), "{operators:?}");
        let (resources, _) = stripped.get_page_resources(page_id)?;
        let resources = resources.ok_or("page lost its resources")?;
        assert!(resources.get(b"Font").is_ok(), "the font resource was lost");
        let xobjects = resources.get(b"XObject")?.as_dict()?;
        assert!(xobjects.is_empty(), "an image XObject resource remained");
        assert!(
            !stripped.objects.values().any(|object| object
                .as_stream()
                .is_ok_and(|stream| stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|subtype| subtype == b"Image"))),
            "an image object remained in the document"
        );
        Ok(())
    }
}
