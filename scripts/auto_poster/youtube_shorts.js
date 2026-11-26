import {
  launchPlatformContext,
  parsePosterArgs,
  replaceContentEditableText,
  validateVideoFile,
} from './common.js';

async function waitForStudioReady(page) {
  await page.goto('https://studio.youtube.com/', { waitUntil: 'domcontentloaded', timeout: 60000 });
  await page.waitForTimeout(2000);

  if (page.url().includes('accounts.google.com') || page.url().includes('signin')) {
    console.log('YouTube Studio login required. Please sign in manually in the opened browser window...');
    await page.waitForURL('https://studio.youtube.com/**', { timeout: 0 });
    await page.waitForTimeout(2000);
    console.log('YouTube Studio login detected.');
  }
}

async function run() {
  const parsed = parsePosterArgs(process.argv);
  if (parsed.mode === 'help') {
    console.log('Usage: node youtube_shorts.js --login-only | --meta <meta.json> | <videoPath> <title> <desc>');
    return;
  }

  const { context, page } = await launchPlatformContext('youtube');

  try {
    await waitForStudioReady(page);

    if (parsed.mode === 'login') {
      console.log('YouTube Studio session saved successfully.');
      await page.waitForTimeout(2500);
      return;
    }

    const { videoPath, title, description } = parsed.meta;
    validateVideoFile(videoPath);

    console.log(`Uploading YouTube Short: ${videoPath}`);

    // Click Create -> Upload videos
    const createBtn = page.locator('ytcp-button#create-icon, ytcp-icon-button[aria-label="Create"], #create-icon').first();
    await createBtn.waitFor({ state: 'visible', timeout: 30000 });
    await createBtn.click();

    const uploadItem = page.locator('tp-yt-paper-item:has-text("Upload videos"), ytcp-ve:has-text("Upload videos")').first();
    await uploadItem.waitFor({ state: 'visible', timeout: 15000 });
    await uploadItem.click();

    // Select file via file chooser
    const [fileChooser] = await Promise.all([
      page.waitForEvent('filechooser', { timeout: 30000 }),
      page.locator('ytcp-button#select-files-button, #select-files-button').first().click(),
    ]);
    await fileChooser.setFiles(videoPath);

    // Wait for metadata editor
    await page.locator('#title-textarea #textbox').first().waitFor({ state: 'visible', timeout: 45000 });

    const safeTitle = (title || 'Viral Short').slice(0, 99);
    const safeDesc = description || `${safeTitle} #shorts`;

    console.log('Filling YouTube Shorts title and description...');
    await replaceContentEditableText(page, page.locator('#title-textarea #textbox').first(), safeTitle);
    await replaceContentEditableText(page, page.locator('#description-textarea #textbox').first(), safeDesc);

    // Audience: Not made for kids
    const notForKids = page.locator('tp-yt-paper-radio-button[name="VIDEO_MADE_FOR_KIDS_NOT_MFK"]').first();
    if (await notForKids.isVisible()) {
      await notForKids.click();
    }

    // Advance through Details -> Video elements -> Checks -> Visibility
    for (let step = 0; step < 3; step++) {
      const nextBtn = page.locator('#next-button').first();
      await nextBtn.waitFor({ state: 'visible', timeout: 15000 });
      await nextBtn.click();
      await page.waitForTimeout(1200);
    }

    // Select Public visibility
    const publicRadio = page.locator('tp-yt-paper-radio-button[name="PUBLIC"]').first();
    await publicRadio.waitFor({ state: 'visible', timeout: 15000 });
    await publicRadio.click();
    await page.waitForTimeout(800);

    // Publish
    console.log('Publishing YouTube Short...');
    const doneBtn = page.locator('#done-button').first();
    await doneBtn.waitFor({ state: 'visible', timeout: 15000 });
    await doneBtn.click();

    await page.locator('ytcp-video-share-dialog, ytcp-uploads-still-processing-dialog').first().waitFor({
      state: 'visible',
      timeout: 60000,
    });
    console.log('Successfully published to YouTube Shorts!');
  } catch (err) {
    console.error('YouTube Shorts upload failed:', err?.message || err);
    process.exitCode = 1;
  } finally {
    await context.close();
  }
}

await run();
