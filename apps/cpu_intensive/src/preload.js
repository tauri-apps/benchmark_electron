const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("electron", {
  onProcessComplete: () => {
    ipcRenderer.send("process-complete")
  }
});
