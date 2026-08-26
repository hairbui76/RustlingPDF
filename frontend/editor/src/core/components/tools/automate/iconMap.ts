import { materialSymbol } from "@app/components/shared/LocalIcon";

const SettingsIcon = materialSymbol("settings-rounded");
const CompressIcon = materialSymbol("compress-rounded");
const SwapHorizIcon = materialSymbol("swap-horiz-rounded");
const CleaningServicesIcon = materialSymbol("cleaning-services-rounded");
const CropIcon = materialSymbol("crop-rounded");
const TextFieldsIcon = materialSymbol("text-fields-rounded");
const PictureAsPdfIcon = materialSymbol("picture-as-pdf-rounded");
const EditIcon = materialSymbol("edit-rounded");
const DeleteIcon = materialSymbol("delete-rounded");
const FolderIcon = materialSymbol("folder-rounded");
const CloudIcon = materialSymbol("cloud");
const StorageIcon = materialSymbol("storage-rounded");
const SearchIcon = materialSymbol("search-rounded");
const DownloadIcon = materialSymbol("download-rounded");
const UploadIcon = materialSymbol("upload-rounded");
const PlayArrowIcon = materialSymbol("play-arrow-rounded");
const RotateLeftIcon = materialSymbol("rotate-left-rounded");
const RotateRightIcon = materialSymbol("rotate-right-rounded");
const VisibilityIcon = materialSymbol("visibility-rounded");
const ContentCutIcon = materialSymbol("content-cut-rounded");
const ContentCopyIcon = materialSymbol("content-copy-rounded");
const WorkIcon = materialSymbol("work");
const BuildIcon = materialSymbol("build-rounded");
const AutoAwesomeIcon = materialSymbol("auto-awesome-rounded");
const SmartToyIcon = materialSymbol("smart-toy-rounded");
const CheckIcon = materialSymbol("check-rounded");
const SecurityIcon = materialSymbol("security-rounded");
const StarIcon = materialSymbol("star-rounded");

export const iconMap = {
  SettingsIcon,
  CompressIcon,
  SwapHorizIcon,
  CleaningServicesIcon,
  CropIcon,
  TextFieldsIcon,
  PictureAsPdfIcon,
  EditIcon,
  DeleteIcon,
  FolderIcon,
  CloudIcon,
  StorageIcon,
  SearchIcon,
  DownloadIcon,
  UploadIcon,
  PlayArrowIcon,
  RotateLeftIcon,
  RotateRightIcon,
  VisibilityIcon,
  ContentCutIcon,
  ContentCopyIcon,
  WorkIcon,
  BuildIcon,
  AutoAwesomeIcon,
  SmartToyIcon,
  CheckIcon,
  SecurityIcon,
  StarIcon,
};

export const iconOptions = [
  { value: "SettingsIcon", label: "Settings" },
  { value: "CompressIcon", label: "Compress" },
  { value: "SwapHorizIcon", label: "Convert" },
  { value: "CleaningServicesIcon", label: "Clean" },
  { value: "CropIcon", label: "Crop" },
  { value: "TextFieldsIcon", label: "Text" },
  { value: "PictureAsPdfIcon", label: "PDF" },
  { value: "EditIcon", label: "Edit" },
  { value: "DeleteIcon", label: "Delete" },
  { value: "FolderIcon", label: "Folder" },
  { value: "CloudIcon", label: "Cloud" },
  { value: "StorageIcon", label: "Storage" },
  { value: "SearchIcon", label: "Search" },
  { value: "DownloadIcon", label: "Download" },
  { value: "UploadIcon", label: "Upload" },
  { value: "PlayArrowIcon", label: "Play" },
  { value: "RotateLeftIcon", label: "Rotate Left" },
  { value: "RotateRightIcon", label: "Rotate Right" },
  { value: "VisibilityIcon", label: "View" },
  { value: "ContentCutIcon", label: "Cut" },
  { value: "ContentCopyIcon", label: "Copy" },
  { value: "WorkIcon", label: "Work" },
  { value: "BuildIcon", label: "Build" },
  { value: "AutoAwesomeIcon", label: "Magic" },
  { value: "SmartToyIcon", label: "Robot" },
  { value: "CheckIcon", label: "Check" },
  { value: "SecurityIcon", label: "Security" },
  { value: "StarIcon", label: "Star" },
];

export type IconKey = keyof typeof iconMap;
