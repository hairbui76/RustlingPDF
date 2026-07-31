const REPOSITORY_URL = "https://github.com/hairbui76/RustlingPDF";
const RELEASES_URL = `${REPOSITORY_URL}/releases`;
const RELEASES_API_URL =
  "https://api.github.com/repos/hairbui76/RustlingPDF/releases";

export interface UpdateSummary {
  latest_version: string | null;
  latest_stable_version?: string | null;
  max_priority: "urgent" | "normal" | "minor" | "low";
  recommended_action?: string;
  any_breaking: boolean;
  migration_guides?: Array<{
    version: string;
    notes: string;
    url: string;
  }>;
}

export interface VersionUpdate {
  version: string;
  priority: "urgent" | "normal" | "minor" | "low";
  announcement: {
    title: string;
    message: string;
  };
  compatibility: {
    breaking_changes: boolean;
    breaking_description?: string;
    migration_guide_url?: string;
  };
}

export interface FullUpdateInfo {
  latest_version: string;
  latest_stable_version?: string;
  new_versions: VersionUpdate[];
}

export interface MachineInfo {
  machineType: string;
}

interface GitHubRelease {
  tag_name: string;
  name: string | null;
  body: string | null;
  html_url: string;
  draft: boolean;
  prerelease: boolean;
}

function normalizeVersion(version: string): string {
  return version.trim().replace(/^v/i, "").split("-", 1)[0] ?? "";
}

function releaseVersion(release: GitHubRelease): string {
  return normalizeVersion(release.tag_name);
}

function isBreaking(body: string | null): boolean {
  return /\bbreaking(?:\s+change)?s?\b/i.test(body ?? "");
}

export class UpdateService {
  /**
   * Compare dotted numeric versions. Pre-release suffixes are ignored because
   * update discovery only consumes stable GitHub releases.
   */
  compareVersions(version1: string, version2: string): number {
    const v1 = normalizeVersion(version1).split(".");
    const v2 = normalizeVersion(version2).split(".");

    for (let i = 0; i < v1.length || i < v2.length; i++) {
      const n1 = Number.parseInt(v1[i] ?? "0", 10) || 0;
      const n2 = Number.parseInt(v2[i] ?? "0", 10) || 0;

      if (n1 > n2) return 1;
      if (n1 < n2) return -1;
    }

    return 0;
  }

  getDownloadUrl(machineInfo: MachineInfo): string | null {
    if (
      machineInfo.machineType === "Docker" ||
      machineInfo.machineType === "Kubernetes"
    ) {
      return null;
    }
    return `${RELEASES_URL}/latest`;
  }

  async getUpdateSummary(
    currentVersion: string,
    _machineInfo: MachineInfo,
  ): Promise<UpdateSummary | null> {
    const releases = await this.fetchStableReleases();
    const latest = releases[0];
    if (!latest) return null;

    const latestVersion = releaseVersion(latest);
    return {
      latest_version: latestVersion,
      latest_stable_version: latestVersion,
      max_priority: "normal",
      recommended_action:
        this.compareVersions(latestVersion, currentVersion) > 0
          ? "Review the release notes, then update when convenient."
          : undefined,
      any_breaking: isBreaking(latest.body),
    };
  }

  async getFullUpdateInfo(
    currentVersion: string,
    _machineInfo: MachineInfo,
  ): Promise<FullUpdateInfo | null> {
    const releases = await this.fetchStableReleases();
    const latest = releases[0];
    if (!latest) return null;

    const newVersions = releases
      .filter(
        (release) =>
          this.compareVersions(releaseVersion(release), currentVersion) > 0,
      )
      .map<VersionUpdate>((release) => {
        const breaking = isBreaking(release.body);
        return {
          version: releaseVersion(release),
          priority: "normal",
          announcement: {
            title: release.name || release.tag_name,
            message: release.body ?? "",
          },
          compatibility: {
            breaking_changes: breaking,
            breaking_description: breaking
              ? (release.body ?? undefined)
              : undefined,
            migration_guide_url: release.html_url,
          },
        };
      });

    return {
      latest_version: releaseVersion(latest),
      latest_stable_version: releaseVersion(latest),
      new_versions: newVersions,
    };
  }

  async getCurrentVersionFromGitHub(): Promise<string> {
    const releases = await this.fetchStableReleases();
    return releases[0] ? releaseVersion(releases[0]) : "";
  }

  private async fetchStableReleases(): Promise<GitHubRelease[]> {
    try {
      const response = await fetch(`${RELEASES_API_URL}?per_page=20`, {
        headers: { Accept: "application/vnd.github+json" },
      });
      if (!response.ok) {
        console.warn(
          `[UpdateService] GitHub releases request failed: ${response.status}`,
        );
        return [];
      }
      const releases = (await response.json()) as GitHubRelease[];
      return releases.filter(
        (release) =>
          !release.draft &&
          !release.prerelease &&
          /^\d+\.\d+\.\d+(?:[-+].*)?$/.test(releaseVersion(release)),
      );
    } catch (error) {
      console.warn("[UpdateService] Could not fetch GitHub releases:", error);
      return [];
    }
  }
}

export const updateService = new UpdateService();
