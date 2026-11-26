import {
  launchPlatformContext,
  parsePosterArgs,
  replaceContentEditableText,
  validateVideoFile,
} from './common.js';

async function waitForInstagramReady(page) {
  await page.goto('https://www.instagram.com/', { waitUntil: 'domcontentloaded', timeout: 60000 });
  await page.waitForTimeout(2500);

  const needsLogin = page.url().includes('/accounts/login')
    || (await page.locator('input[name="username"]').isVisible().catch(() => false));

  if (needsLogin) {
    console.log('Instagram login required. Please sign in manually in the opened browser window...');
    await page.waitForFunction(
      () => !window.location.href.includes('/accounts/login')
        && !document.querySelector('input[name="username"]'),
      { timeout: 0 },
    );
    await page.waitForTimeout(2500);
    console.log('Instagram login detected.');
  }

  // Dismiss "Not Now" notifications / save login prompts if present
  const notNowBtn = page.locator('button:has-text("Not Now"), button:has-text("Not now")').first();
  if (await notNowBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
    await notNowBtn.click();
  }
}

async function run() {
  const parsed = parsePosterArgs(process.argv);
  if (parsed.mode === 'help') {
    console.log('Usage: node instagram_reels.js --login-only | --meta <meta.json> | <videoPath> <title> <desc>');
    return;
  }

  const { context, page } = await launchPlatformContext('instagram');

  try {
    await waitForInstagramReady(page);

    if (parsed.mode === 'login') {
      console.log('Instagram session saved successfully.');
      await page.waitForTimeout(2500);
      return;
    }

    const { videoPath, instagramCaption, title } = parsed.meta;
    validateVideoFile(videoPath);

    console.log(`Uploading Instagram Reel: ${videoPath}`);

    // Open Create modal from left navigation
    const createNav = page
      .locator('a:has-text("Create"), span:has-text("Create"), svg[aria-label="New post"]')
      .first();
    await createNav.waitFor({ state: 'visible', timeout: 30000 });
    await createNav.click();
    await page.waitForTimeout(1000);

    // Some accounts show a submenu ("Post" / "Live video") after clicking Create
    const postSubItem = page.locator('a:has-text("Post"), span:has-text("Post")').first();
    if (await postSubItem.isVisible({ timeout: 2000 }).catch(() => false)) {
      await postSubItem.click();
      await page.waitForTimeout(800);
    }

    // Upload video file directly into the modal file input
    const fileInput = page.locator('div[role="dialog"] input[type="file"], input[type="file"]').first();
    await fileInput.waitFor({ state: 'attached', timeout: 30000 });
    await fileInput.setInputFiles(videoPath);
    await page.waitForTimeout(2500);

    // Dismiss "Video posts are now shared as reels" info dialog if shown
    const okBtn = page.locator('div[role="dialog"] button:has-text("OK")').first();
    if (await okBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await okBtn.click();
      await page.waitForTimeout(800);
    }

    // Keep 9:16 vertical aspect ratio by choosing Original / 9:16 if crop toggle is available
    const cropBtn = page.locator('div[role="dialog"] button svg[aria-label="Select crop"]').first();
    if (await cropBtn.isVisible({ timeout: 2500 }).catch(() => false)) {
      await cropBtn.click();
      await page.waitForTimeout(500);
      const originalOpt = page.locator('div[role="dialog"] button:has-text("Original"), div[role="dialog"] span:has-text("9:16")').first();
      if (await originalOpt.isVisible({ timeout: 2000 }).catch(() => false)) {
        await originalOpt.click();
      }
    }

    // Click Next twice (Crop -> Trim/Cover -> Caption)
    for (let step = 0; step < 2; step++) {
      const nextBtn = page.locator('div[role="dialog"] div[role="button"]:has-text("Next"), div[role="dialog"] button:has-text("Next")').first();
      await nextBtn.waitFor({ state: 'visible', timeout: 30000 });
      await nextBtn.click();
      await page.waitForTimeout(1500);
    }

    // Fill Reels caption
    const captionBox = page
      .locator('div[role="dialog"] div[aria-label="Write a caption..."], div[role="dialog"] div[contenteditable="true"]')
      .first();
    await captionBox.waitFor({ state: 'visible', timeout: 30000 });

    const captionText = (instagramCaption || `${title || 'Viral Reel'}\n\n#reels #viral`).slice(0, 2200);
    console.log('Setting Instagram Reels caption...');
    await replaceContentEditableText(page, captionBox, captionText);
    await page.waitForTimeout(1000);

    // Click Share
    console.log('Publishing Instagram Reel...');
    const shareBtn = page
      .locator('div[role="dialog"] div[role="button"]:has-text("Share"), div[role="dialog"] button:has-text("Share")')
      .first();
    await shareBtn.waitFor({ state: 'visible', timeout: 20000 });
    await shareBtn.click();

    // Wait for upload confirmation
    await page
      .locator('text=/Your reel has been shared|Reel shared|Post shared/i')
      .first()
      .waitFor({ state: 'visible', timeout: 120000 });
    console.log('Successfully published to Instagram Reels!');
  } catch (err) {
    console.error('Instagram Reels upload failed:', err?.message || err);
    process.exitCode = 1;
  } finally {
    await context.close();
  }
}

await run();
