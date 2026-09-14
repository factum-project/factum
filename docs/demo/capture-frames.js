const puppeteer = require('puppeteer');
const fs = require('fs');
const path = require('path');

async function captureFrames() {
  const htmlPath = path.resolve(__dirname, 'terminal-animation.html');
  const framesDir = path.resolve(__dirname, 'frames');
  
  // Clean up old frames
  if (fs.existsSync(framesDir)) {
    fs.rmSync(framesDir, { recursive: true });
  }
  fs.mkdirSync(framesDir, { recursive: true });

  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-setuid-sandbox']
  });

  const page = await browser.newPage();
  await page.setViewport({ width: 620, height: 380, deviceScaleFactor: 2 });
  await page.goto('file://' + htmlPath, { waitUntil: 'networkidle0' });

  console.log('Capturing frames...');
  
  // Total animation time: ~28 lines * avg ~300ms = ~8.4s + 3s pause + loop
  // Capture at 15fps for ~12 seconds = 180 frames
  const totalFrames = 180;
  const frameInterval = 1000 / 15; // 15fps
  
  for (let i = 0; i < totalFrames; i++) {
    const framePath = path.join(framesDir, `frame_${String(i).padStart(4, '0')}.png`);
    await page.screenshot({
      path: framePath,
      clip: { x: 0, y: 0, width: 620, height: 380 }
    });
    
    if (i % 30 === 0) {
      console.log(`  Frame ${i}/${totalFrames}`);
    }
    
    await new Promise(resolve => setTimeout(resolve, frameInterval));
  }

  console.log('Frames captured. Closing browser.');
  await browser.close();
}

captureFrames().catch(console.error);
