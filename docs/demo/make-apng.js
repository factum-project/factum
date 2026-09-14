const UPNG = require('upng-js');
const fs = require('fs');
const path = require('path');

async function createAPNG() {
  const framesDir = path.resolve(__dirname, 'frames');
  const outputPath = path.resolve(__dirname, 'factum-mcp-demo.png');

  const files = fs.readdirSync(framesDir)
    .filter(f => f.endsWith('.png'))
    .sort();

  console.log(`Found ${files.length} frames`);

  // Read all frames as ArrayBuffers
  const frames = [];
  const widths = new Set();
  const heights = new Set();

  for (let i = 0; i < files.length; i++) {
    const filePath = path.join(framesDir, files[i]);
    const buf = fs.readFileSync(filePath);
    const img = UPNG.decode(buf);
    widths.add(img.width);
    heights.add(img.height);

    // UPNG.toRGBA8 returns array of ArrayBuffers (one per frame)
    const rgba = UPNG.toRGBA8(img);
    // For single-frame PNG, rgba[0] is the frame data
    frames.push(rgba[0]);

    if (i % 20 === 0) {
      console.log(`  Decoded frame ${i}/${files.length} (${img.width}x${img.height})`);
    }
  }

  const width = [...widths][0];
  const height = [...heights][0];
  console.log(`Dimensions: ${width}x${height}, ${frames.length} frames`);

  // Create APNG
  // UPNG.encode(frames, w, h, cnum, delay)
  // frames: array of ArrayBuffers
  // cnum: 0 = lossless, >0 = lossy palette
  // delay: array of delays in ms per frame
  const delays = new Array(frames.length).fill(100); // 100ms = 10fps

  console.log('Encoding APNG (lossless)...');
  const apng = UPNG.encode(frames, width, height, 0, delays);

  fs.writeFileSync(outputPath, Buffer.from(apng));

  const stats = fs.statSync(outputPath);
  console.log(`APNG created: ${outputPath}`);
  console.log(`Dimensions: ${width}x${height}`);
  console.log(`Frames: ${frames.length}`);
  console.log(`Size: ${(stats.size / 1024 / 1024).toFixed(2)} MB`);
}

createAPNG().catch(console.error);
