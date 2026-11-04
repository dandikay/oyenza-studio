# Oyenza Studio: WhatsApp Web (browser extension)

Shows WhatsApp Web messages on the Oyenza Studio island, in WhatsApp's green,
with who it's from and the message.

It works with Brave, Chrome, Edge and other Chromium browsers (111 or newer)
and with the **WhatsApp Web** add-on of Oyenza Studio for Windows.

## Install

1. In Oyenza Studio, click the **+** at the end of the tabs and switch on
   **WhatsApp Web**.
2. In your browser open `brave://extensions` (Chrome: `chrome://extensions`,
   Edge: `edge://extensions`) and switch on **Developer mode**.
3. Click **Load unpacked** and pick this folder (`extensions/whatsapp-web`).
4. Open <https://web.whatsapp.com> (reload it if it was already open), sign
   in, and keep the tab open.

A message that comes while you aren't looking at that chat shows on the island
for 8 seconds. Muted chats stay quiet, as in WhatsApp.

## What it does and doesn't do

- It wraps the page's `Notification` call: it hands the notification's title
  and text to Oyenza Studio on `http://127.0.0.1:47653` (this PC only), and
  lets the notification show as usual if your browser allows it. To have the
  island as the only place, turn notifications for web.whatsapp.com off in the
  browser's site settings.
- It reads nothing else on the page: no chat list, no contacts, no media.
- It sends nothing anywhere but your own PC. Oyenza Studio accepts only
  requests that come from a browser extension, carrying this extension's header.
- With Oyenza Studio closed, or the add-on off, nothing listens and the
  messages are dropped.
