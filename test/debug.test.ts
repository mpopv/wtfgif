import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import { GifReader as OmgGifReader } from "omggif";
import { GifReader as WtfGifReader } from "../src/index";

const gifsDir = join(__dirname, "gifs");
const testFiles = ["18d30677-d255-4cc9-9933-c8d35306c1d5.gif", "Clap-1x.gif"];

describe("Pixel Difference Investigation", () => {
  for (const file of testFiles) {
    test(`${file} - analyze pixel differences`, () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);
      const omgAny = omg as any;
      const wtfAny = wtf as any;

      console.log(`\n=== ${file} ===`);
      console.log(
        `Dimensions: ${omg.width}x${omg.height}, Frames: ${omg.numFrames()}`
      );
      console.log(`Loop count: omg=${omg.loopCount()}, wtf=${wtf.loopCount()}`);

      // Check global color table
      console.log(
        `Global palette: omg=${
          omgAny.globalColorTable !== null
        }, wtf=${wtfAny.hasGlobalPalette?.()}`
      );
      console.log(
        `Background: omg=${omgAny.bgColor}, wtf=${wtfAny.backgroundIndex?.()}`
      );

      const pixelCount = omg.width * omg.height * 4;

      // Test frame 0 only for detailed analysis
      const frameIdx = 0;
      const omgFrame = omg.frameInfo(frameIdx);
      const omgFrameAny = omgFrame as any;
      const wtfFrame = wtf.frameInfo(frameIdx);

      console.log(`\nFrame ${frameIdx} info:`);
      console.log(
        `Position: omg=(${omgFrame.x},${omgFrame.y}), wtf=(${wtfFrame.x},${wtfFrame.y})`
      );
      console.log(
        `Size: omg=${omgFrame.width}x${omgFrame.height}, wtf=${wtfFrame.width}x${wtfFrame.height}`
      );
      console.log(
        `Disposal: omg=${omgFrame.disposal}, wtf=${wtfFrame.disposal}`
      );
      console.log(`Delay: omg=${omgFrame.delay}, wtf=${wtfFrame.delay}`);
      console.log(
        `Transparent: omg=${omgFrame.transparent_index}, wtf=${wtfFrame.transparent_index}`
      );
      console.log(
        `Interlaced: omg=${omgFrame.interlaced}, wtf=${wtfFrame.interlaced}`
      );
      console.log(
        `Local palette: omg=${omgFrameAny.localColorTable !== null}, wtf=${
          wtfFrame.has_local_palette
        }`
      );

      // Compare actual palettes being used
      if (omgFrameAny.localColorTable !== null && wtfFrame.has_local_palette) {
        console.log(
          `  Both have local palettes - comparing first few entries:`
        );
        if (Array.isArray(omgFrameAny.localColorTable)) {
          for (
            let p = 0;
            p < Math.min(5, omgFrameAny.localColorTable.length);
            p++
          ) {
            console.log(
              `    Entry ${p}: omg=0x${omgFrameAny.localColorTable[p]
                .toString(16)
                .padStart(6, "0")}`
            );
          }
        }
        // wtfgif doesn't expose local palette directly, so we can't compare easily
      }

      // Check palette mismatches
      const omgHasLocal = omgFrameAny.localColorTable !== null;
      const wtfHasLocal = wtfFrame.has_local_palette;
      const omgHasGlobal = omgAny.globalColorTable !== null;
      const wtfHasGlobal =
        typeof wtfAny.hasGlobalPalette === "function"
          ? wtfAny.hasGlobalPalette()
          : false;

      console.log(`  Palette mismatch analysis:`);
      console.log(`    Local: omg=${omgHasLocal}, wtf=${wtfHasLocal}`);
      console.log(`    Global: omg=${omgHasGlobal}, wtf=${wtfHasGlobal}`);

      if (omgHasLocal !== wtfHasLocal || omgHasGlobal !== wtfHasGlobal) {
        console.log(`    *** PALETTE DETECTION MISMATCH ***`);
      }

      if (
        typeof wtfAny.hasGlobalPalette === "function" &&
        wtfAny.hasGlobalPalette() &&
        omgAny.globalColorTable
      ) {
        console.log(
          `  Both have global palettes - comparing first few entries:`
        );
        const wtfGlobal =
          typeof wtfAny.globalPalette === "function"
            ? wtfAny.globalPalette()
            : undefined;
        for (let p = 0; p < Math.min(5, omgAny.globalColorTable.length); p++) {
          const omgColor = omgAny.globalColorTable[p];
          const wtfColor = wtfGlobal ? wtfGlobal[p] : -1;
          console.log(
            `    Entry ${p}: omg=0x${omgColor
              .toString(16)
              .padStart(6, "0")}, wtf=0x${wtfColor
              .toString(16)
              .padStart(6, "0")}`
          );
        }
      }

      // Decode both
      const omgRGBA = new Uint8Array(pixelCount);
      const wtfRGBA = new Uint8Array(pixelCount);
      omg.decodeAndBlitFrameRGBA(frameIdx, omgRGBA);
      wtf.decodeAndBlitFrameRGBA(frameIdx, wtfRGBA);

      // Count differences
      let diffCount = 0;
      let firstDiffIdx = -1;
      const maxDiffsToShow = 10;
      let diffsShown = 0;

      for (let i = 0; i < pixelCount; i++) {
        if (omgRGBA[i] !== wtfRGBA[i]) {
          if (firstDiffIdx === -1) firstDiffIdx = i;
          diffCount++;

          if (diffsShown < maxDiffsToShow) {
            const pixelIdx = Math.floor(i / 4);
            const component = ["R", "G", "B", "A"][i % 4];
            const x = pixelIdx % omg.width;
            const y = Math.floor(pixelIdx / omg.width);
            console.log(
              `  Diff ${diffsShown + 1}: pixel(${x},${y}) ${component}: omg=${
                omgRGBA[i]
              } wtf=${wtfRGBA[i]}`
            );
            diffsShown++;
          }
        }
      }

      const diffPercent = ((diffCount / pixelCount) * 100).toFixed(2);
      console.log(
        `\nTotal differences: ${diffCount}/${pixelCount} (${diffPercent}%)`
      );

      if (diffCount > 0) {
        console.log(`First difference at byte ${firstDiffIdx}`);

        // Look for patterns in differences
        const alphaChannelDiffs = [];
        const colorChannelDiffs = [];

        for (let i = 0; i < Math.min(pixelCount, 1000); i++) {
          if (omgRGBA[i] !== wtfRGBA[i]) {
            if (i % 4 === 3) {
              // Alpha channel
              alphaChannelDiffs.push({ i, omg: omgRGBA[i], wtf: wtfRGBA[i] });
            } else {
              // Color channels
              colorChannelDiffs.push({ i, omg: omgRGBA[i], wtf: wtfRGBA[i] });
            }
          }
        }

        console.log(
          `Alpha channel diffs in first 1000 bytes: ${alphaChannelDiffs.length}`
        );
        console.log(
          `Color channel diffs in first 1000 bytes: ${colorChannelDiffs.length}`
        );

        if (alphaChannelDiffs.length > 0) {
          console.log(`Sample alpha diffs:`, alphaChannelDiffs.slice(0, 3));
        }
        if (colorChannelDiffs.length > 0) {
          console.log(`Sample color diffs:`, colorChannelDiffs.slice(0, 3));
        }
      }

      wtf.returnToPool();

      // For debugging, fail if differences found
      // if (diffCount > 0) {
      //   throw new Error(`Found ${diffCount} pixel differences - investigation needed`);
      // }
    });
  }

  test("direct palette usage comparison", () => {
    const file = "Clap-1x.gif"; // This one has the biggest mismatch
    const gif = readFileSync(join(gifsDir, file));
    const omg = new OmgGifReader(gif);
    const wtf = new WtfGifReader(gif);

    console.log(`\n=== Direct Palette Analysis for ${file} ===`);

    // For omggif: examine what it actually exposes
    const omgAny = omg as any;
    const wtfAny = wtf as any;
    console.log(
      `omggif globalColorTable type:`,
      typeof omgAny.globalColorTable
    );
    console.log(
      `omggif globalColorTable length:`,
      omgAny.globalColorTable?.length
    );
    if (omgAny.globalColorTable && omgAny.globalColorTable.length > 0) {
      console.log(
        `omggif global palette first 5:`,
        (omgAny.globalColorTable as number[])
          .slice(0, 5)
          .map((c: number) => `0x${c.toString(16).padStart(6, "0")}`)
      );
    }

    // For frame 0 (the problematic one)
    const omgFrame0 = omg.frameInfo(0);
    const omgFrame0Any = omgFrame0 as any;
    console.log(
      `omggif frame 0 localColorTable type:`,
      typeof omgFrame0Any.localColorTable
    );
    console.log(
      `omggif frame 0 localColorTable length:`,
      omgFrame0Any.localColorTable?.length
    );
    if (
      omgFrame0Any.localColorTable &&
      omgFrame0Any.localColorTable.length > 0
    ) {
      console.log(
        `omggif local palette first 5:`,
        (omgFrame0Any.localColorTable as number[])
          .slice(0, 5)
          .map((c: number) => `0x${c.toString(16).padStart(6, "0")}`)
      );
    }

    // For wtfgif: examine what it exposes
    const hasGlobalPalette =
      typeof wtfAny.hasGlobalPalette === "function"
        ? wtfAny.hasGlobalPalette()
        : false;
    console.log(`wtfgif hasGlobalPalette:`, hasGlobalPalette);
    if (hasGlobalPalette) {
      const wtfGlobal =
        typeof wtfAny.globalPalette === "function"
          ? wtfAny.globalPalette()
          : undefined;
      console.log(`wtfgif global palette length:`, wtfGlobal?.length);
      if (wtfGlobal && wtfGlobal.length > 0) {
        console.log(
          `wtfgif global palette first 5:`,
          (wtfGlobal as number[])
            .slice(0, 5)
            .map((c: number) => `0x${c.toString(16).padStart(6, "0")}`)
        );
      }
    }

    const wtfFrame0 = wtf.frameInfo(0);
    console.log(
      `wtfgif frame 0 has_local_palette:`,
      wtfFrame0.has_local_palette
    );

    // The key question: Which palette is each actually using when decoding?
    // Let's decode a single pixel and see what color comes out
    const singlePixelRGBA = new Uint8Array(4);
    const singlePixelWtf = new Uint8Array(4);

    // Decode just first pixel of frame 0
    const fullOmg = new Uint8Array(omg.width * omg.height * 4);
    const fullWtf = new Uint8Array(wtf.width * wtf.height * 4);
    omg.decodeAndBlitFrameRGBA(0, fullOmg);
    wtf.decodeAndBlitFrameRGBA(0, fullWtf);

    // Extract first pixel
    singlePixelRGBA[0] = fullOmg[0]; // R
    singlePixelRGBA[1] = fullOmg[1]; // G
    singlePixelRGBA[2] = fullOmg[2]; // B
    singlePixelRGBA[3] = fullOmg[3]; // A

    singlePixelWtf[0] = fullWtf[0]; // R
    singlePixelWtf[1] = fullWtf[1]; // G
    singlePixelWtf[2] = fullWtf[2]; // B
    singlePixelWtf[3] = fullWtf[3]; // A

    console.log(
      `First pixel decoded: omg=RGB(${singlePixelRGBA[0]},${singlePixelRGBA[1]},${singlePixelRGBA[2]}), wtf=RGB(${singlePixelWtf[0]},${singlePixelWtf[1]},${singlePixelWtf[2]})`
    );

    wtf.returnToPool();
  });

  test("transparency handling comparison", () => {
    const file = testFiles[0];
    const gif = readFileSync(join(gifsDir, file));
    const omg = new OmgGifReader(gif);
    const wtf = new WtfGifReader(gif);

    // Look specifically at transparency handling
    for (
      let frameIdx = 0;
      frameIdx < Math.min(omg.numFrames(), 2);
      frameIdx++
    ) {
      const omgFrame = omg.frameInfo(frameIdx);
      const wtfFrame = wtf.frameInfo(frameIdx);

      console.log(`\nFrame ${frameIdx} transparency analysis:`);
      console.log(
        `  Has transparent index: omg=${
          omgFrame.transparent_index !== null
        }, wtf=${wtfFrame.transparent_index !== null}`
      );
      console.log(
        `  Transparent index: omg=${omgFrame.transparent_index}, wtf=${wtfFrame.transparent_index}`
      );

      if (
        omgFrame.transparent_index !== null ||
        wtfFrame.transparent_index !== null
      ) {
        // Decode a small area to check transparency handling
        const pixelCount = 64; // 8x8 area for analysis
        const omgRGBA = new Uint8Array(omg.width * omg.height * 4);
        const wtfRGBA = new Uint8Array(wtf.width * wtf.height * 4);

        omg.decodeAndBlitFrameRGBA(frameIdx, omgRGBA);
        wtf.decodeAndBlitFrameRGBA(frameIdx, wtfRGBA);

        // Check first few pixels for transparency
        let transparentPixelsOmg = 0;
        let transparentPixelsWtf = 0;

        for (let i = 0; i < Math.min(pixelCount, omg.width * omg.height); i++) {
          const alphaOmg = omgRGBA[i * 4 + 3];
          const alphaWtf = wtfRGBA[i * 4 + 3];

          if (alphaOmg === 0) transparentPixelsOmg++;
          if (alphaWtf === 0) transparentPixelsWtf++;
        }

        console.log(
          `  Transparent pixels in first ${pixelCount}: omg=${transparentPixelsOmg}, wtf=${transparentPixelsWtf}`
        );
      }
    }

    wtf.returnToPool();
  });
});
