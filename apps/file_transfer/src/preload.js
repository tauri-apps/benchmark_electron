const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("electron", {
  readFile: () => {
    ipcRenderer.send("read-file");
  }
});
