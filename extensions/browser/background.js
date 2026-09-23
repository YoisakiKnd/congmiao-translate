const HOST = "app.congmiao.translate";

function requestNative(message) {
  return new Promise((resolve, reject) => {
    let port;
    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (error) {
      reject(error);
      return;
    }
    const fail = () => {
      const reason = chrome.runtime.lastError?.message || "无法连接从喵翻译";
      reject(new Error(`${reason}\n启动从喵翻译：congmiao://translate`));
    };
    port.onDisconnect.addListener(fail);
    port.onMessage.addListener((response) => {
      port.onDisconnect.removeListener(fail);
      resolve(response);
    });
    port.postMessage(message);
  });
}

function nativeResult(response) {
  if (response?.type === "error") {
    return { ok: false, message: response.message || "翻译失败" };
  }
  if (response?.type === "compare_result") {
    const first = (response.results || []).find((item) => item.text && !item.error);
    return {
      ok: true,
      text: first?.text || "",
      results: response.results || [],
      detectedSource: response.detected_source || null,
    };
  }
  return {
    ok: true,
    text: response.text,
    cacheHit: Boolean(response.cache_hit),
    results: response.text ? [{ engine: "default", label: "译文", text: response.text }] : [],
  };
}

async function translate(message) {
  const response = await requestNative({
    type: "compare",
    id: crypto.randomUUID(),
    text: message.text,
    source: message.source ?? null,
    target: message.target || "zh",
  });
  return nativeResult(response);
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus?.create({
    id: "congmiao-translate",
    title: "用从喵翻译",
    contexts: ["selection"],
  });
});

chrome.contextMenus?.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== "congmiao-translate" || !tab?.id || !info.selectionText) return;
  chrome.tabs.sendMessage(tab.id, {
    type: "show-translation",
    text: info.selectionText,
  });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message) return;
  if (message.type === "batch") {
    requestNative({
      type: "batch",
      id: crypto.randomUUID(),
      texts: message.texts || [],
      source: message.source ?? null,
      target: message.target || "zh",
    })
      .then((response) => {
        if (response?.type === "error") sendResponse({ ok: false, message: response.message });
        else sendResponse({ ok: true, texts: response.texts || [] });
      })
      .catch((error) => sendResponse({ ok: false, message: error.message }));
    return true;
  }
  if (message.type !== "translate" && message.type !== "compare") return;
  translate(message)
    .then(sendResponse)
    .catch((error) => sendResponse({ ok: false, message: error.message }));
  return true;
});
