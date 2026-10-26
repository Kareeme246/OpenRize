/** Per-release download counters, from GitHub's public release API. */
export interface ReleaseCounts {
  tag: string;
  /** The macOS installer fetched straight from the Releases page. */
  dmg: number;
  /** The Windows installer fetched straight from the Releases page. */
  exe: number;
  /** `openrize.app.tar.gz`: in-app updates installed plus `install.sh` runs, not website downloads. */
  updates: number;
  /** `latest.json`: the app's update checks, not downloads. */
  updateChecks: number;
}

interface ApiRelease {
  tag_name: string;
  draft: boolean;
  assets: { name: string; download_count: number }[];
}

const sumOf = (
  assets: ApiRelease["assets"],
  match: (name: string) => boolean,
) =>
  assets
    .filter((asset) => match(asset.name))
    .reduce((sum, asset) => sum + asset.download_count, 0);

/** Published releases, newest first. GITHUB_TOKEN, when set, only raises the rate limit. */
export async function fetchReleaseCounts(
  env: NodeJS.ProcessEnv = process.env,
  send: typeof fetch = fetch,
): Promise<ReleaseCounts[]> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github+json",
    "User-Agent": "openrize-download-stats",
  };
  if (env.GITHUB_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
  const response = await send(
    "https://api.github.com/repos/Kareeme246/OpenRize/releases?per_page=100",
    { headers },
  );
  if (!response.ok) {
    throw new Error(`GitHub releases: HTTP ${response.status}`);
  }
  const releases = (await response.json()) as ApiRelease[];
  return releases
    .filter((release) => !release.draft)
    .map((release) => ({
      tag: release.tag_name,
      dmg: sumOf(release.assets, (name) => name.endsWith(".dmg")),
      exe: sumOf(release.assets, (name) => name.endsWith("-setup.exe")),
      updates: sumOf(release.assets, (name) => name === "openrize.app.tar.gz"),
      updateChecks: sumOf(release.assets, (name) => name === "latest.json"),
    }));
}
