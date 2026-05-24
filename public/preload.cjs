const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("klyppd", {
  invoke: (channel, ...args) => ipcRenderer.invoke(channel, ...args),
  on: (channel, callback) => {
    const listener = (_event, ...args) => callback(...args);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  },
  clipboard: {
    writeText: (text) => ipcRenderer.invoke("clipboard:writeText", text),
  },
  window: {
    hide: () => ipcRenderer.invoke("window:hide"),
    close: () => ipcRenderer.invoke("window:close"),
  },
});
