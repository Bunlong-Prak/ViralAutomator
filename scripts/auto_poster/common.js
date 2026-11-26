import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export const SELECT_ALL_KEY = process.platform === 'darwin' ? 'Meta+A' : 'Control+A';

export function getProfileDir(platform) {
  const root = path.join(__dirname, '..', '..', '.auth_profiles');
  fs.mkdirSync(root, { recursive: true });
  return path.join(root, platform);
}

export function parsePosterArgs(argv) {
  const args = argv.slice(2);
  if (args.includes('--help') || args.includes('-h')) {
    return { mode: 'help' };
  }
  if (args.includes('--login-only')) {
    return { mode: 'login' };
  }

  const metaIdx = args.indexOf('--meta');
  if (metaIdx !== -1 && args[metaIdx + 1]) {
    const metaPath = args[metaIdx + 1];
    if (!fs.existsSync(metaPath)) {
      throw new Error(`Metadata JSON file not found: ${metaPath}`);
    }
    const raw = fs.readFileSync(metaPath, 'utf8');
    const meta = JSON.parse(raw);
    return { mode: 'upload', meta };
  }

  const [videoPath, title, description] = args;
  if (!videoPath) {
    throw new Error('Missing --meta <file.json>, --login-only, or <videoPath> <title> <description>');
  }

  return {
    mode: 'upload',
    meta: {
      videoPath,
      title: title || 'Viral Short',
      description: description || `${title || 'Viral Short'} #shorts`,
      tiktokCaption: `${title || 'Viral Short'} #fyp #viral`,
      instagramCaption: `${description || title || 'Viral Short'} #reels`,
      hashtags: ['shorts', 'fyp', 'reels', 'viral'],
    },
  };
}

export async function launchPlatformContext(platform) {
  const userDataDir = getProfileDir(platform);
  const context = await chromium.launchPersistentContext(userDataDir, {
    headless: false,
    viewport: { width: 1280, height: 860 },
    args: [
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-blink-features=AutomationControlled',
    ],
  });
  const pages = context.pages();
  const page = pages.length > 0 ? pages[0] : await context.newPage();
  return { context, page };
}

export async function replaceContentEditableText(page, locator, text) {
  await locator.waitFor({ state: 'visible', timeout: 30000 });
  await locator.click();
  await page.keyboard.press(SELECT_ALL_KEY);
  await page.keyboard.press('Backspace');
  await page.keyboard.insertText(text);
}

export function validateVideoFile(videoPath) {
  if (!videoPath || !fs.existsSync(videoPath)) {
    throw new Error(`Video file not found: ${videoPath}`);
  }
}
