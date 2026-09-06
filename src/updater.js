// Minimal static-frontend bridge for Tauri's updater plugin.
(() => {
  const invoke = window.__TAURI__.core.invoke;
  const SERIALIZE_TO_IPC_FN = "__TAURI_TO_IPC_KEY__";

  class Channel {
    constructor(onmessage) {
      this.onmessage = onmessage || (() => {});
      this.nextIndex = 0;
      this.pending = new Map();
      this.id = window.__TAURI_INTERNALS__.transformCallback((raw) => {
        if (raw && "end" in raw) return;
        const index = raw?.index ?? this.nextIndex;
        const message = raw?.message;
        if (index === this.nextIndex) {
          this.onmessage(message);
          this.nextIndex++;
          while (this.pending.has(this.nextIndex)) {
            this.onmessage(this.pending.get(this.nextIndex));
            this.pending.delete(this.nextIndex++);
          }
        } else {
          this.pending.set(index, message);
        }
      });
    }
    [SERIALIZE_TO_IPC_FN]() { return `__CHANNEL__:${this.id}`; }
    toJSON() { return this[SERIALIZE_TO_IPC_FN](); }
  }

  window.__NWC_UPDATER__ = {
    check: () => invoke("plugin:updater|check"),
    downloadAndInstall: (update, onEvent) => invoke("plugin:updater|download_and_install", {
      rid: update.rid,
      onEvent: new Channel(onEvent),
      restartAfterInstall: true,
    }),
  };
})();
