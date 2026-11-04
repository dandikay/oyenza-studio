// Posts each WhatsApp Web message to Oyenza Studio, which listens on this PC
// only (127.0.0.1, the WhatsApp Web add-on). If the app or the add-on is off,
// nothing listens and the message is simply dropped.
const ENDPOINT = 'http://127.0.0.1:47653/chat';
const PAGE = 'https://web.whatsapp.com';

// Where a message came from. `sender.tab.url` is empty without the "tabs"
// permission, so use the origin, which is always there.
function originOf(sender) {
  if (sender.origin) return sender.origin;
  try {
    return new URL(sender.url).origin;
  } catch (_) {
    return '';
  }
}

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!sender.tab || originOf(sender) !== PAGE) {
    console.log('oyenza: ignored a message from', originOf(sender) || 'unknown');
    return;
  }
  const one = (s) => String(s || '').replace(/[\r\n]+/g, ' ').trim();
  const from = one(msg.from);
  const text = String(msg.text || '').trim();
  if (!from && !text) return;
  fetch(ENDPOINT, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain', 'X-Oyenza-Client': 'wa-web' },
    body: from + '\n' + text,
  })
    .then((r) => console.log('oyenza: island answered', r.status))
    .catch((e) => console.log('oyenza: island not reachable', String(e)));
});
