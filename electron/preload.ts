import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("klyppd", {
  invoke: (channel: string, ...args: unknown[]) =>
    ipcRenderer.invoke(channel, ...args),
  on: (channel: string, callback: (...args: unknown[]) => void) => {
    const listener = (_event: Electron.IpcRendererEvent, ...args: unknown[]) =>
      callback(...args);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  },
  clipboard: {
    writeText: (text: string) => ipcRenderer.invoke("clipboard:writeText", text),
  },
  window: {
    hide: () => ipcRenderer.invoke("window:hide"),
    close: () => ipcRenderer.invoke("window:close"),
  },
});
