use crate::utils::add_log;
use std::sync::Mutex;

/// One batch of opened file paths plus the optional tool intent they arrived
/// with (from an Explorer context-menu `--tool <action>` launch). Plain opens
/// (double-click, drag-drop, macOS open event) carry `tool: None`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFileBatch {
    pub paths: Vec<String>,
    pub tool: Option<String>,
}

// Store the opened batches globally (supports multiple files and launches).
// The intent lives in this queue (not in an event payload) so it survives a
// cold launch where the frontend mounts after the batch was enqueued.
static OPENED_BATCHES: Mutex<Vec<OpenedFileBatch>> = Mutex::new(Vec::new());

fn flatten(batches: &[OpenedFileBatch]) -> Vec<String> {
    batches
        .iter()
        .flat_map(|batch| batch.paths.iter().cloned())
        .collect()
}

// Add an opened file path with no tool intent. Consecutive intent-less adds
// coalesce into one trailing batch; an intent batch is never appended to.
pub fn add_opened_file(file_path: String) {
    let mut batches = OPENED_BATCHES.lock().unwrap();
    match batches.last_mut() {
        Some(last) if last.tool.is_none() => last.paths.push(file_path.clone()),
        _ => batches.push(OpenedFileBatch {
            paths: vec![file_path.clone()],
            tool: None,
        }),
    }
    add_log(format!("📂 File stored for later retrieval: {}", file_path));
}

// Add a whole batch (paths + optional intent) as one unit.
pub fn add_opened_batch(paths: Vec<String>, tool: Option<String>) {
    if paths.is_empty() {
        return;
    }
    add_log(format!(
        "📂 Batch stored for later retrieval: {} file(s), intent {:?}",
        paths.len(),
        tool
    ));
    let mut batches = OPENED_BATCHES.lock().unwrap();
    batches.push(OpenedFileBatch { paths, tool });
}

// Command to get opened file paths (if app was launched with files).
// Non-destructive, intent-agnostic view kept for backwards compatibility.
#[tauri::command]
pub async fn get_opened_files() -> Result<Vec<String>, String> {
    let batches = OPENED_BATCHES.lock().unwrap();
    let all_files = flatten(&batches);
    add_log(format!("📂 Returning {} opened file(s)", all_files.len()));
    Ok(all_files)
}

// Command to clear the opened files (after processing)
#[tauri::command]
pub async fn clear_opened_files() -> Result<(), String> {
    let mut batches = OPENED_BATCHES.lock().unwrap();
    batches.clear();
    add_log("📂 Cleared opened files".to_string());
    Ok(())
}

// Command to atomically get and clear opened file paths. Backwards-compatible
// flattened view: intents are dropped. Prefer `pop_opened_batches`.
#[tauri::command]
pub async fn pop_opened_files() -> Result<Vec<String>, String> {
    let mut batches = OPENED_BATCHES.lock().unwrap();
    let all_files = flatten(&batches);
    batches.clear();
    add_log(format!(
        "📂 Returning and clearing {} opened file(s)",
        all_files.len()
    ));
    Ok(all_files)
}

// Command to atomically get and clear the opened batches, preserving each
// batch's tool intent. This is what the desktop frontend consumes.
#[tauri::command]
pub async fn pop_opened_batches() -> Result<Vec<OpenedFileBatch>, String> {
    let mut batches = OPENED_BATCHES.lock().unwrap();
    let all_batches = std::mem::take(&mut *batches);
    add_log(format!(
        "📂 Returning and clearing {} opened batch(es)",
        all_batches.len()
    ));
    Ok(all_batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(paths: &[&str], tool: Option<&str>) -> OpenedFileBatch {
        OpenedFileBatch {
            paths: paths.iter().map(|p| p.to_string()).collect(),
            tool: tool.map(|t| t.to_string()),
        }
    }

    // Single test over the shared global queue: the scenarios must run
    // sequentially, not as parallel #[test] threads racing OPENED_BATCHES.
    #[tokio::test]
    async fn queue_intent_semantics() {
        clear_opened_files().await.unwrap();

        // Intent-less adds coalesce into one trailing batch.
        add_opened_file("/a.pdf".to_string());
        add_opened_file("/b.pdf".to_string());
        // An intent batch is stored as its own unit.
        add_opened_batch(
            vec!["/c.pdf".to_string(), "/d.pdf".to_string()],
            Some("merge".to_string()),
        );
        // A later plain add must NOT be folded into the intent batch.
        add_opened_file("/e.pdf".to_string());
        // Empty batches are ignored.
        add_opened_batch(Vec::new(), Some("compress".to_string()));

        // Non-destructive flattened view (twice: it must not consume).
        let expected_flat = vec!["/a.pdf", "/b.pdf", "/c.pdf", "/d.pdf", "/e.pdf"];
        assert_eq!(get_opened_files().await.unwrap(), expected_flat);
        assert_eq!(get_opened_files().await.unwrap(), expected_flat);

        // Destructive batch view preserves grouping, order, and intents.
        let batches = pop_opened_batches().await.unwrap();
        assert_eq!(
            batches,
            vec![
                batch(&["/a.pdf", "/b.pdf"], None),
                batch(&["/c.pdf", "/d.pdf"], Some("merge")),
                batch(&["/e.pdf"], None),
            ]
        );
        assert!(pop_opened_batches().await.unwrap().is_empty());

        // Back-compat flattened pop drops intents but returns every path.
        add_opened_batch(vec!["/f.pdf".to_string()], Some("convert".to_string()));
        add_opened_file("/g.pdf".to_string());
        assert_eq!(pop_opened_files().await.unwrap(), vec!["/f.pdf", "/g.pdf"]);
        assert!(pop_opened_files().await.unwrap().is_empty());

        // clear_opened_files empties intent batches too.
        add_opened_batch(vec!["/h.pdf".to_string()], Some("merge".to_string()));
        clear_opened_files().await.unwrap();
        assert!(pop_opened_batches().await.unwrap().is_empty());
    }
}
