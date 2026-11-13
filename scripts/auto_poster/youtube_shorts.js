const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const [,, videoPath, title, description] = process.argv;

if (!videoPath || !title || !description) {
  console.error('Usage: node youtube_shorts.js <videoPath> <title> <description>');
  process.exit(1);
}

if (!fs.existsSync(videoPath)) {
  console.error(`❌ Video file not found: ${videoPath}`);
  process.exit(1);
}

(async () => {
  console.log(`Starting YouTube Shorts auto-post for: ${videoPath}`);

  // Use persistent context to retain login cookies
  const userDataDir = path.join(__dirname, '..', '..', '.youtube_auth');

  // Headless is currently set to false. On the very first run, you will need to manually sign in to Google.
  // After the first run, cookies will be saved in .youtube_auth and you can safely change this to true.
  const browser = await chromium.launchPersistentContext(userDataDir, {
    headless: false,
    args: ['--no-sandbox', '--disable-setuid-sandbox'],
  });

  const page = await browser.newPage();

  try {
    await page.goto('https://studio.youtube.com/', { waitUntil: 'networkidle' });

    // Check if we need to login
    if (page.url().includes('accounts.google.com')) {
      console.log('NOT LOGGED IN! Please login manually in the opened browser window.');
      console.log('Waiting for user to complete login...');
      await page.waitForURL('https://studio.youtube.com/**', { timeout: 0 }); // Wait indefinitely
      console.log('Login detected. Proceeding...');
    }

    console.log('Navigating to upload...');

    // Click Create button
    await page.locator('ytcp-icon-button[aria-label="Create"]').click();
    await page.locator('tp-yt-paper-item:has-text("Upload videos")').click();

    // Upload file
    console.log('Uploading file...');
    const [fileChooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.locator('ytcp-button#select-files-button').click(),
    ]);
    await fileChooser.setFiles(videoPath);

    // Wait for the upload form to appear
    await page.waitForSelector('#title-textarea', { state: 'visible' });

    console.log('Setting title and description...');

    const selectAllKey = process.platform === 'darwin' ? 'Meta+A' : 'Control+A';

    // YouTube's new contenteditable textboxes require specific click/fill sequences
    await page.locator('#title-textarea #textbox').click();
    await page.keyboard.press(selectAllKey);
    await page.keyboard.press('Backspace');
    await page.keyboard.insertText(title);

    await page.locator('#description-textarea #textbox').click();
    await page.keyboard.press(selectAllKey);
    await page.keyboard.press('Backspace');
    await page.keyboard.insertText(description);

    // Mark as "Not made for kids" (required)
    await page.locator('tp-yt-paper-radio-button[name="VIDEO_MADE_FOR_KIDS_NOT_MFK"]').click();

    console.log('Waiting for upload to finish processing...');

    // Click Next until Visibility tab
    for (let i = 0; i < 3; i++) {
      await page.locator('#next-button').click();
      await page.waitForTimeout(1000);
    }

    // Select Public
    await page.locator('tp-yt-paper-radio-button[name="PUBLIC"]').click();

    // Publish
    console.log('Publishing...');
    await page.locator('#done-button').click();

    // Wait for success dialog
    await page.waitForSelector('ytcp-video-share-dialog', { state: 'visible' });
    console.log('✅ Successfully published to YouTube Shorts!');
  } catch (err) {
    console.error('❌ Error during upload:', err);
    process.exitCode = 1;
  } finally {
    console.log('Closing browser...');
    await browser.close();
  }
})();
