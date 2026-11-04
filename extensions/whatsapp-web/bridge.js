// Runs in the extension's own world on the WhatsApp Web page: passes what
// inject.js caught to the background script. Only messages from this very
// page (and only the shape inject.js sends) go on.
window.addEventListener('message', (event) => {
  const d = event.data;
  if (event.source !== window || !d || d.source !== 'oyenza-wa') return;
  if (typeof d.title !== 'string' || typeof d.body !== 'string') return;
  try {
    chrome.runtime.sendMessage({ from: d.title, text: d.body }).catch(() => {});
  } catch (_) {}
});
