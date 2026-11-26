import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PublishStudioView from '../../src/views/app/PublishStudioView.vue';
import { i18n } from '../../src/i18n';
import { defaultSettings } from '../../src/tauri/types/config';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock('../../src/stores/settings', () => ({
  useSettingsStore: () => ({
    settings: structuredClone(defaultSettings),
  }),
}));

vi.mock('../../src/composables/useDialog', () => ({
  useDialog: () => ({
    openPathDialog: vi.fn().mockResolvedValue(null),
  }),
}));

vi.mock('../../src/composables/useOpener', () => ({
  useOpener: () => ({
    revealPath: vi.fn(),
  }),
}));

describe('PublishStudioView', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    invokeMock.mockReset();
  });

  it('loads queue and platform auth status and renders clip cards', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'publish_queue_list') {
        return Promise.resolve([
          {
            id: 'clip-1',
            videoPath: '/tmp/short_1.mp4',
            thumbnailPath: null,
            sourceTitle: 'Podcast Viral Moment',
            sourceUrl: 'https://youtube.com/watch?v=123',
            viralityScore: 94,
            hookText: 'NOBODY TALKS ABOUT THIS',
            reason: 'Strong opening hook',
            youtubeTitle: 'Why 99% Fail at Automation',
            youtubeDescription: 'Full breakdown #shorts',
            tiktokCaption: 'Agree or disagree? #fyp',
            instagramCaption: 'Save this! #reels',
            hashtags: ['shorts', 'fyp'],
            scheduledAt: null,
            createdAt: 1700000000,
            youtube: { enabled: true, status: 'pending', lastError: null, postedAt: null },
            tiktok: { enabled: true, status: 'scheduled', lastError: null, postedAt: null },
            instagram: { enabled: true, status: 'posted', lastError: null, postedAt: 1700001000 },
          },
        ]);
      }
      if (cmd === 'publish_auth_status') {
        return Promise.resolve({
          youtubeConnected: true,
          tiktokConnected: false,
          instagramConnected: true,
        });
      }
      return Promise.resolve([]);
    });

    const wrapper = mount(PublishStudioView, {
      global: {
        plugins: [i18n],
        stubs: {
          RouterLink: RouterLinkStub,
          BaseSubNav: {
            template: '<div><slot name="title" /><slot /></div>',
          },
        },
      },
    });

    await flushPromises();

    expect(wrapper.text()).toContain('Viral Publish Studio');
    expect(wrapper.text()).toContain('Why 99% Fail at Automation');
    expect(wrapper.text()).toContain('94/100 Viral');
    expect(wrapper.text()).toContain('NOBODY TALKS ABOUT THIS');
    expect(wrapper.find('dialog[aria-labelledby="publish-editor-title"]').exists()).toBe(true);
  });
});
