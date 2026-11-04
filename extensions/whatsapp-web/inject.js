// Runs inside the WhatsApp Web page (world "MAIN"), before its own code.
//
// WhatsApp Web shows a message with `new Notification(title, { body })`.
// This wraps that call: the title (who it's from) and body (the message)
// go to bridge.js, and the notification is shown as before when the
// browser allows it. It also answers "granted" to WhatsApp's permission
// check, so WhatsApp makes the notification even where the browser has
// notifications for the site off: the island is then the only place the
// message shows. Muted chats make no notification, so they stay quiet.
(() => {
  const Native = window.Notification;
  if (!Native || Native.__oyenza) return;

  const send = (title, body) => {
    try {
      window.postMessage({ source: 'oyenza-wa', title: String(title == null ? '' : title), body: String(body == null ? '' : body) }, window.location.origin);
    } catch (_) {}
  };

  // What a notification the browser won't show returns: nothing happens.
  const inert = () => {
    const t = new EventTarget();
    t.close = () => {};
    return t;
  };

  function Wrapped(title, options) {
    send(title, options && options.body);
    if (Native.permission === 'granted') {
      try {
        return new Native(title, options);
      } catch (_) {}
    }
    return inert();
  }
  Wrapped.prototype = Native.prototype;
  Wrapped.__oyenza = true;
  Wrapped.maxActions = Native.maxActions;
  Object.defineProperty(Wrapped, 'permission', { get: () => 'granted' });
  Wrapped.requestPermission = (callback) => {
    if (typeof callback === 'function') callback('granted');
    return Promise.resolve('granted');
  };
  window.Notification = Wrapped;

  // Some versions of WhatsApp Web notify through the service worker.
  const proto = window.ServiceWorkerRegistration && window.ServiceWorkerRegistration.prototype;
  if (proto && proto.showNotification && !proto.showNotification.__oyenza) {
    const showNative = proto.showNotification;
    proto.showNotification = function (title, options) {
      send(title, options && options.body);
      if (Native.permission === 'granted') return showNative.call(this, title, options);
      return Promise.resolve();
    };
    proto.showNotification.__oyenza = true;
  }
})();
