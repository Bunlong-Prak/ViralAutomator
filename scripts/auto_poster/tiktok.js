import {
  launchPlatformContext,
  parsePosterArgs,
  replaceContentEditableText,
  validateVideoFile,
} from './common.js';

async function waitForTikTokStudioReady(page) {
  await page.goto('https://www.tiktok.com/tiktokstudio/upload?from=webapp', {
    waitUntil: 'domcontentloaded',
    timeout: 60000,
  });
  await page.waitForTimeout(2500);

  if (page.url().includes('/login')) {
    console.log('TikTok login required. Please sign in manually in the opened browser window...');
    await page.waitForURL(url => !url.toString().includes('/login'), { timeout: 0 });
    await page.goto('https://www.tiktok.com/tiktokstudio/upload?from=webapp', {
      waitUntil: 'domcontentloaded',
      timeout: 60000,
    });
    await page.waitForTimeout(2500);
    console.log('TikTok login detected.');
  }
}

async function run() {
  const parsed = parsePosterArgs(process.argv);
  if (parsed.mode === 'help') {
    console.log('Usage: node tiktok.js --login-only | --meta <meta.json> | <videoPath> <title> <desc>');
    return;
  }

  const { context, page } = await launchPlatformContext('tiktok');

  try {
    await waitForTikTokStudioReady(page);

    if (parsed.mode === 'login') {
      console.log('TikTok Studio session saved successfully.');
      await page.waitForTimeout(2500);
      return;
    }

    const { videoPath, tiktokCaption, title } = parsed.meta;
    validateVideoFile(videoPath);

    console.log(`Uploading TikTok video: ${videoPath}`);

    // TikTok Studio uses a hidden file input[type="file"] for video uploads
    const fileInput = page.locator('input[type="file"][accept*="video"], input[type="file"]').first();
    await fileInput.waitFor({ state: 'attached', timeout: 45000 });
    await fileInput.setInputFiles(videoPath);

    // Wait for caption editor to appear once upload begins
    const captionEditor = page
      .locator('[contenteditable="true"].public-DraftEditor-content, div[contenteditable="true"][role="combobox"], div[contenteditable="true"]')
      .first();
    await captionEditor.waitFor({ state: 'visible', timeout: 60000 });

    const captionText = (tiktokCaption || `${title || 'Viral Clip'} #fyp #viral`).slice(0, 2200);
    console.log('Setting TikTok caption and hashtags...');
    await replaceContentEditableText(page, captionEditor, captionText);

    // Allow video transcode / copyright check to settle before clicking Post
    await page.waitForTimeout(4000);

    const postBtn = page
      .locator('button[data-e2e="post_video_button"], button:has-text("Post")')
      .last();
    await postBtn.waitFor({ state: 'visible', timeout: 60000 });

    // Wait until Post button becomes enabled after upload completes
    await page.waitForFunction(
      btn => btn && !btn.disabled && btn.getAttribute('aria-disabled') !== 'true',
      await postBtn.elementHandle(),
      { timeout: 120000 },
    );

    console.log('Publishing TikTok video...');
    await postBtn.click();

    // Confirm post modal if TikTok asks "Continue to post?"
    const confirmBtn = page.locator('button:has-text("Post now"), button:has-text("Continue")').first();
    if (await confirmBtn.isVisible({ timeout: 5000 }).catch(() => false)) {
      await confirmBtn.click();
    }

    await page.waitForTimeout(5000);
    console.log('Successfully submitted video to TikTok!');
  } catch (err) {
    console.error('TikTok upload failed:', err?.message || err);
    process.exitCode = 1;
  } finally {
    await context.close();
  }
}

await run();
