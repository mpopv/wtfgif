const fs = require('fs');
const path = require('path');
const sharp = require('sharp');
const { GifReader } = require('./wtfgif.js');

// Extract frames as PNG images using sharp
async function extractFrames() {
  const gifsDir = './test/gifs';
  const framesDir = './test/gifs/frames';
  
  // Create frames directory if it doesn't exist
  if (!fs.existsSync(framesDir)) {
    fs.mkdirSync(framesDir, { recursive: true });
  }
  
  // Get all GIF files
  const gifFiles = fs.readdirSync(gifsDir).filter(file => 
    file.toLowerCase().endsWith('.gif') && fs.statSync(path.join(gifsDir, file)).isFile()
  );
  
  console.log(`Found ${gifFiles.length} GIF files to process`);
  
  for (const gifFile of gifFiles) {
    try {
      const gifPath = path.join(gifsDir, gifFile);
      const gifData = fs.readFileSync(gifPath);
      const reader = new GifReader(gifData);
      
      const baseName = path.basename(gifFile, '.gif');
      const gifFramesDir = path.join(framesDir, baseName);
      
      if (!fs.existsSync(gifFramesDir)) {
        fs.mkdirSync(gifFramesDir, { recursive: true });
      }
      
      // Extract each frame as PNG
      const frameInfos = [];
      for (let i = 0; i < reader.numFrames(); i++) {
        const frameInfo = reader.frameInfo(i);
        
        // Decode frame to RGBA pixels
        const pixels = new Uint8Array(reader.width * reader.height * 4);
        reader.decodeAndBlitFrameRGBA(i, pixels);
        
        // Save as PNG using sharp
        const framePath = path.join(gifFramesDir, `frame_${i.toString().padStart(3, '0')}.png`);
        await sharp(Buffer.from(pixels), {
          raw: {
            width: reader.width,
            height: reader.height,
            channels: 4
          }
        })
        .png()
        .toFile(framePath);
        
        frameInfos.push({
          index: i,
          filename: `frame_${i.toString().padStart(3, '0')}.png`,
          x: frameInfo.x,
          y: frameInfo.y,
          width: frameInfo.width,
          height: frameInfo.height,
          delay: frameInfo.delay,
          disposal: frameInfo.disposal,
          transparent_index: frameInfo.transparent_index
        });
      }
      
      
      console.log(`Processed ${gifFile}: ${reader.numFrames()} frames, ${reader.width}x${reader.height}`);
      
    } catch (error) {
      console.error(`Error processing ${gifFile}:`, error.message);
    }
  }
}

extractFrames().catch(console.error);