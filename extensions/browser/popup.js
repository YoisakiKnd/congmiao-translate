const target = document.querySelector("#target");
const text = document.querySelector("#text");
const result = document.querySelector("#result");

chrome.storage.local.get({ targetLang: "zh" }, (saved) => {
  target.value = saved.targetLang || "zh";
});

target.addEventListener("change", () => {
  chrome.storage.local.set({ targetLang: target.value });
});

document.querySelector("#go").addEventListener("click", async () => {
  const value = text.value.trim();
  if (!value) {
    result.textContent = "先输入要翻译的文字";
    return;
  }
  result.textContent = "正在翻译…";
  const response = await chrome.runtime.sendMessage({
    type: "translate",
    text: value,
    source: "auto",
    target: target.value,
  });
  result.textContent = response?.ok
    ? (response.results || []).map((item) => item.error || item.text).filter(Boolean).join("\n") || response.text
    : response?.message || "翻译失败。若桌面端没启动，打开 congmiao://translate";
});
