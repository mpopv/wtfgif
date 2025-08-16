export function hashGifData(data: Uint8Array): string {
  let hash = 0;
  const step = Math.max(1, Math.floor(data.length / 1024));
  for (let i = 0; i < data.length; i += step) {
    hash = ((hash << 5) - hash + data[i]) | 0;
  }
  return hash.toString(36);
}
