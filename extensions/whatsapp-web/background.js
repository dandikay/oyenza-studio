// Posts each WhatsApp Web message to Oyenza Studio, which listens on this PC
// only (127.0.0.1, the WhatsApp Web add-on). If the app or the add-on is off,
// nothing listens and the message is simply dropped.
const ENDPOINT = 'http://127.0.0.1:47653/chat';

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!sender.tab || !sender.tab.url || !sender.tab.url.startsWith('https://web.whatsapp.com/')) return;
  const one = (s) => String(s || '').replace(/[\r\n]+/g, ' ').trim();
  const from = one(msg.from);
  const text = String(msg.text || '').trim();
  if (!from && !text) return;
  fetch(ENDPOINT, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain', 'X-Oyenza-Client': 'wa-web' },
    body: from + '\n' + text,
  }).catch(() => {});
});
