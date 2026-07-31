import React from "react";
import { RustlingFileStub } from "@app/types/fileContext";
import { PrivateContent } from "@app/components/shared/PrivateContent";
import { truncateCenter } from "@app/utils/textUtils";

interface FileEditorFileNameProps {
  file: RustlingFileStub;
  maxLength?: number;
}

const FileEditorFileName = ({
  file,
  maxLength = 40,
}: FileEditorFileNameProps) => (
  <PrivateContent>{truncateCenter(file.name, maxLength)}</PrivateContent>
);

export default FileEditorFileName;
