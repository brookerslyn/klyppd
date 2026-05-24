interface KlyppdBridge {
  invoke: (channel: string, ...args: unknown[]) => Promise<unknown>;
  on: (
    channel: string,
    callback: (...args: unknown[]) => void
  ) => () => void;
  clipboard: { writeText: (text: string) => Promise<void> };
  window: { hide: () => Promise<void>; close: () => Promise<void> };
}

interface Window {
  klyppd: KlyppdBridge;
}
