const GIFEncoder = require('gif-encoder-2');
const sharp = require('sharp');
const fs = require('fs');
const path = require('path');

async function createGif() {
  const framesDir = path.resolve(__dirname, 'frames');
  const outputPath = path.resolve(__dirname, 'factum-mcp-demo.gif');
  
  const files = fs.readdirSync(framesDir)
    .filter(f => f.endsWith('.png'))
    .sort();
  
  console.log(`Found ${files.length} frames`);
  
  // Downscale to 620x380 for smaller GIF
  const width = 620;
  const height = 380;
  
  const encoder = new GIFEncoder(width, height);
  encoder.setQuality(10);
  encoder.setDelay(67); // ~15fps
  encoder.setRepeat(0); // loop forever
  encoder.setTransparent(false);
  
  const gifStream = fs.createWriteStream(outputPath);
  encoder.createReadStream().pipe(gifStream);
  
  encoder.start();
  
  for (let i = 0; i < files.length; i++) {
    const filePath = path.join(framesDir, files[i]);
    const { data } = await sharp(filePath)
      .resize(width, height)
      .raw()
      .toBuffer({ resolveWithObject: true });
    
    encoder.addFrame(data);
    
    if (i % 30 === 0) {
      console.log(`  Encoding frame ${i}/${files.length}`);
    }
  }
  
  encoder.finish();
  
  return new Promise((resolve) => {
    gifStream.on('finish', () => {
      const stats = fs.statSync(outputPath);
      console.log(`GIF created: ${outputPath}`);
      console.log(`Size: ${(stats.size / 1024 / 1024).toFixed(2)} MB`);
      resolve();
    });
  });
}

createGif().catch(console.error);
