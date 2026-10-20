export class FileReadMeasurements {
  constructor() {
    this.assets = new Map();
    this.reads = 0;
    this.bytes = 0;
    this.evictedAssetStats = 0;
  }
  record(file, offset, length, kind) {
    const asset = String(file.id);
    const previous = this.assets.get(asset);
    this.assets.delete(asset);
    if (this.assets.size === 16) {
      this.assets.delete(this.assets.keys().next().value);
      this.evictedAssetStats++;
    }
    const entry = {
      asset,
      path: file.path.slice(0, 512),
      pathTruncated: file.path.length > 512,
      reads: (previous?.reads ?? 0) + 1,
      bytes: (previous?.bytes ?? 0) + length,
    };
    this.assets.set(asset, entry);
    this.reads++;
    this.bytes += length;
    this.lastRead = { asset, path: entry.path, offset: String(offset), length, kind };
  }
  snapshot() {
    return {
      sourceReads: this.reads,
      sourceReadBytes: this.bytes,
      evictedAssetStats: this.evictedAssetStats,
      lastRead: this.lastRead && { ...this.lastRead },
      recentAssets: [...this.assets.values()].map((entry) => ({ ...entry })),
    };
  }
}
