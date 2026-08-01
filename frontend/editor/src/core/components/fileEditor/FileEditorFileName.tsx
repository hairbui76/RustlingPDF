import { RustlingFileStub } from "@app/types/fileContext";
import { truncateCenter } from "@app/utils/textUtils";

interface FileEditorFileNameProps {
  file: RustlingFileStub;
  maxLength?: number;
}

const FileEditorFileName = ({
  file,
  maxLength = 40,
}: FileEditorFileNameProps) => truncateCenter(file.name, maxLength);

export default FileEditorFileName;
