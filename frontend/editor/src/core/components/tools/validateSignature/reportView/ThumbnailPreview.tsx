import "@app/components/tools/validateSignature/reportView/styles.css";
import { LocalIcon } from "@app/components/shared/LocalIcon";

const ThumbnailPreview = ({
  thumbnailUrl,
  fileName,
}: {
  thumbnailUrl?: string | null;
  fileName: string;
}) => {
  if (thumbnailUrl) {
    return (
      <div className="thumbnail-container">
        <img
          src={thumbnailUrl}
          alt={`${fileName} thumbnail`}
          className="thumbnail-image"
        />
      </div>
    );
  }

  return (
    <div className="thumbnail-placeholder">
      <LocalIcon
        icon="picture-as-pdf-rounded"
        width="2.1875rem"
        height="2.1875rem"
      />
    </div>
  );
};

export default ThumbnailPreview;
