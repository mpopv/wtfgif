import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test, expect } from 'vitest';
import { GifReader as OmgGifReader } from 'omggif';
import { GifReader as WtfGifReader } from '../src/index';

test('GifReader dimensions match omggif', () => {
  const gifPath = join(__dirname, 'gifs', 'party_blob.gif');
  const gif = readFileSync(gifPath);
  const omg = new OmgGifReader(gif);
  const wtf = new WtfGifReader(gif);
  expect(wtf.width).toBe(omg.width);
  expect(wtf.height).toBe(omg.height);
});
