import { listen } from '@tauri-apps/api/event';
import { PublishItem } from '../types/publish.ts';
import { usePublishStore } from '../../stores/publish.ts';

export function registerPublishListeners(): void {
  const publishStore = usePublishStore();

  void listen<PublishItem[]>('publish_queue_updated', ({ payload }) => {
    publishStore.setItems(payload);
  });
}
