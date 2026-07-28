const ROOT_TASKFILE: &str = include_str!("../../../../Taskfile.yml");
const ENGINE_TASKFILE: &str = include_str!("../../../../.taskfiles/engine.yml");

#[test]
fn task_surface_is_rust_only() {
    assert!(ROOT_TASKFILE.contains("taskfile: .taskfiles/engine.yml\n    dir: ."));
    assert!(ROOT_TASKFILE.contains("- task: engine:dev"));
    for task in [
        "  prepare:",
        "  dev:",
        "  typecheck:",
        "  lint:fix:",
        "  fix:",
        "  check:",
    ] {
        assert!(ENGINE_TASKFILE.contains(task), "missing public task {task}");
    }
    assert!(ENGINE_TASKFILE.contains("-p rustling-ai-engine"));
    assert!(!ENGINE_TASKFILE.contains("uvicorn"));
    assert!(!ENGINE_TASKFILE.contains("legacy:"));
    assert!(!ENGINE_TASKFILE.contains("dotenv:"));
    assert!(ENGINE_TASKFILE.contains("  tool-models:check:"));
    assert!(ENGINE_TASKFILE.contains("-p rustling-operation-catalog --locked"));
    assert!(!ENGINE_TASKFILE.contains("generate_ai_operation_catalog.py"));
}
